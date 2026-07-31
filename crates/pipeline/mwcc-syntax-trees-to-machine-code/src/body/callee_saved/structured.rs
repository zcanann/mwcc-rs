//! Structured control flow whose register values survive conditional calls.
//!
//! This is the conservative bridge between semantic statement lowering and the
//! virtual-register allocator.  It owns a complete function only when every
//! statement is representable by the shared store/call emitter plus forward
//! `if` branches; unsupported control flow declines before emitting anything.

use super::guarded_computed_survivor::emit_scaled_index;
use super::structured_call_accumulator::{
    call_accumulator_assignment_count, call_accumulator_names,
    fold_zero_initialized_call_accumulator, in_place_call_combined_return_name,
};
use super::structured_aggregate_slots::{
    plan_aggregate_frame_slots, plan_aggregate_frame_slots_from,
    plan_terminal_one_word_aggregate_call_copies,
};
use super::structured_call_schedule::{
    direct_callback_wait_home_preference, terminal_offset_call_argument_register,
    transient_call_argument_register,
};
use super::structured_sequenced_callback_wait::{
    is_sequenced_callback_wait_layout, sequenced_callback_wait_frame_slot,
    sequenced_callback_wait_home_preference, sequenced_callback_wait_save_order,
    sequenced_callback_wait_starter,
};
use super::structured_constant_versions::{
    repeated_store_constant_exceeds_home_capacity,
    retain_repeated_store_constant_across_call,
};
use super::structured_condition_schedule::thread_forward_unconditional_branch_chains;
use super::structured_entry_alias::{
    fold_entry_alias_zero_test, plan_first_call_alias, EntryAliasBoundary, EntryParameterAlias,
};
use super::structured_early_return_schedule::{
    resolve_structured_epilogue_branches, STRUCTURED_EPILOGUE_PLACEHOLDER,
};
use super::structured_exclusive_arm_home_layout::ExclusiveArmHomeLayout;
use super::structured_frame_assignment::{
    adjacent_byte_pointer_round_up_name, fold_adjacent_byte_pointer_round_up,
    fold_terminal_pointer_load_alias, is_folded_terminal_pointer_load_alias,
    is_transient_biased_scaled_member_call_local, is_transient_direct_call_argument_local,
    is_transient_shifted_member_mask_call_local, plan_dense_eager_pointer_round_up,
    sink_low_mask_parameter_assignment, sink_single_use_parameter_assignment,
};
use super::structured_frame_arrays::{
    align_offset, array_stack_alignment, plan_structured_frame_arrays,
    structured_array_placement_order,
};
use super::structured_array_pool::plan_structured_array_pool;
use super::structured_frame_entry::structured_dense_frame_entry_index;
use super::structured_frame_ordinals::pre_constant_label_count;
use super::structured_global_index_cache::plan as plan_structured_global_index_cache;
use super::structured_global_base_cache::plan as plan_structured_global_base_cache;
use super::structured_global_member_address_cache::
    plan as plan_structured_global_member_address_cache;
use super::structured_frame_publication::{
    StructuredFramePublication, CURSOR_OFFSET, LOCAL_REGION_BYTES, OWNER_OFFSET,
};
use super::structured_async_callback_switch_layout::StructuredAsyncCallbackSwitchLayout;
use super::structured_home_layout::{
    allocator_result_cursor_preferences, compact_aggregate_scratch_frame_pair,
    dense_eager_deferred_preferences, dense_eager_home_preference,
    paired_eager_deferred_preference, rounded_pointer_dense_home_preference,
    returned_deferred_pair_preference, saved_float_home_preference,
    uses_rounded_pointer_dense_layout,
};
use super::structured_deferred_local_layout::retains_deferred_saved_local_lane;
use super::structured_indirect_call_home::promote_cost_free_indirect_call_locals;
use super::structured_interleaved_frame_layout::StructuredInterleavedFrameLayout;
use super::structured_liveness::{
    read_after_possible_call, read_after_possible_call_in_function,
    transient_condition_call_result_callee,
};
use super::structured_loop_invariants::hoist_iterator_end_sentinels;
use super::structured_loop_packet_invariants::hoist_repeated_packet_words;
use super::structured_loop_member_cache::cache_repeated_loop_members;
use super::structured_loop_assertion_strings::plan_loop_assertion_strings;
use super::structured_loop_lowering::{
    lower_structured_loops, strip_side_effect_free_empty_switches,
};
use super::structured_repeated_call_poll::is_repeated_call_poll_transaction;
use super::structured_switch_lowering::{
    is_lowered_switch_guard, lower_structured_switches,
    lower_structured_switches_for_emission, resolve_structured_switch_joins,
    shared_base_comparison_switch, structured_switch_join_placeholder,
};
use super::structured_sparse_switch::is_sparse_shared_body_switch;
use super::structured_shared_switch_global_value::{
    plan as plan_structured_shared_switch_global_value,
    SharedSwitchGlobalValueHome,
};
use super::structured_condition_join_cache::{
    followup_after_call_free_join, retained_values_after_join,
};
use super::structured_loop_register_pressure::{
    plan_dense_loop_carried_locals, plan_dense_loop_register_window,
};
use super::structured_loop_member_receiver_layout::StructuredLoopMemberReceiverLayout;
use super::structured_object_collision_loop_layout::StructuredObjectCollisionLoopLayout;
use super::structured_object_collision_loop_schedule::schedule_object_collision_loop_entry;
use super::structured_preloop_alias::fold_preloop_comma_pointer_alias;
use super::structured_precomposition_home_layout::StructuredPrecompositionHomeLayout;
use super::structured_locals::{
    body_uses_local, dead_ephemeral_float_locals, is_frame_address_null_select,
    is_unobserved_local_assignment, plan_deferred_saved_homes,
    plan_distinct_deferred_saved_homes, plan_ephemeral_locals,
};
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
use super::structured_eager_home_reuse::StructuredEagerHomeReuse;
use super::structured_complement_product_pair::StructuredComplementProductPair;
use super::structured_prologue::{
    dense_entry_owns_parameter_copies, saved_home_stores_precede_initialization,
    uses_dense_saved_register_range,
};
use super::structured_register_width::assigned_register_width;
use super::structured_state_transfer_layout::is_unused_array_state_transfer;
use super::structured_value_versions::{
    has_split_value_version, leaf_parameter_mask_version, reassignment_live_source,
    split_reassigned_local_versions,
};
use super::structured_variadic_output_frame::StructuredVariadicOutputFrame;
use super::structured_unobserved_scalar_table::UnobservedScalarTable;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower a void structured body after assigning every value that can be read
    /// following a (possibly conditional) call to a virtual callee-saved home.
    pub(crate) fn try_callee_saved_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let suppressed_constant =
            repeated_store_constant_exceeds_home_capacity(function);
        let mut rewritten = function.clone();
        let mut retained_constant = false;
        while let Some(next) = retain_repeated_store_constant_across_call(&rewritten) {
            rewritten = next;
            retained_constant = true;
        }
        self.try_callee_saved_structured_body_impl(
            if retained_constant { &rewritten } else { function },
            false,
            suppressed_constant,
        )
    }

    /// Route trailing guarded returns through the same structured statement
    /// compiler as source-level early returns. Guards are a compact parser
    /// representation; once calls require whole-body liveness, retaining a
    /// separate lowering path only prevents frame and saved-home planning from
    /// seeing the complete control-flow graph.
    pub(crate) fn try_callee_saved_structured_guard_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.guards.is_empty() {
            return Ok(false);
        }
        let mut normalized = function.clone();
        normalized
            .statements
            .extend(normalized.guards.drain(..).map(|guard| Statement::If {
                condition: guard.condition,
                then_body: vec![Statement::Return(Some(guard.value))],
                else_body: Vec::new(),
            }));
        if self.try_callee_saved_structured_body(&normalized)? {
            self.legacy_callee_saved_frame_layout =
                LegacyCalleeSavedFrameLayout::RetainGuardedEntryParameterTable;
            return Ok(true);
        }
        if self.try_callee_saved_structured_frame_body(&normalized)? {
            self.legacy_callee_saved_frame_layout =
                LegacyCalleeSavedFrameLayout::RetainGuardedEntryParameterTable;
            return Ok(true);
        }
        Ok(false)
    }

    /// The same virtual-register path with uninitialized automatic byte arrays
    /// composed below its saved homes and a shared integer-valued exit.
    pub(crate) fn try_callee_saved_structured_frame_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let mut normalized = function.clone();
        let mut changed = false;
        if let Some(rewritten) = fold_adjacent_byte_pointer_round_up(&normalized) {
            normalized = rewritten;
            changed = true;
        }
        if let Some(rewritten) = fold_terminal_pointer_load_alias(&normalized) {
            normalized = rewritten;
            changed = true;
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            if let Some(rewritten) = sink_low_mask_parameter_assignment(&normalized) {
                normalized = rewritten;
                changed = true;
            } else if let Some(rewritten) = sink_single_use_parameter_assignment(&normalized) {
                normalized = rewritten;
                changed = true;
            }
        }
        if let Some(rewritten) = fold_zero_initialized_call_accumulator(&normalized) {
            normalized = rewritten;
            changed = true;
        }
        if let Some(rewritten) = split_reassigned_local_versions(&normalized) {
            normalized = rewritten;
            changed = true;
        }
        if changed {
            self.try_callee_saved_structured_body_impl(&normalized, true, false)
        } else {
            self.try_callee_saved_structured_body_impl(function, true, false)
        }
    }

    fn try_callee_saved_structured_body_impl(
        &mut self,
        function: &Function,
        with_frame_array: bool,
        suppressed_constant_lane: bool,
    ) -> Compilation<bool> {
        // Macro-expanded display-list packets are an input normalization for
        // this general structured path. More exact semantic owners run before
        // reaching here and retain their original packet statements.
        let coalesced_packets =
            super::super::display_list_packet_runs::coalesce_display_list_packet_runs(function);
        let function = coalesced_packets.as_ref().unwrap_or(function);
        let hoisted_packet_words = hoist_repeated_packet_words(function);
        let function = hoisted_packet_words.as_ref().unwrap_or(function);
        let cached_loop_members = cache_repeated_loop_members(function);
        let function = cached_loop_members.as_ref().unwrap_or(function);
        let hoisted_iterator_end =
            hoist_iterator_end_sentinels(function, &self.one_word_aggregate_locals);
        let function = hoisted_iterator_end.as_ref().unwrap_or(function);
        let folded_preloop_alias = fold_preloop_comma_pointer_alias(function);
        let function = folded_preloop_alias.as_ref().unwrap_or(function);
        let stripped_empty_switches = strip_side_effect_free_empty_switches(function);
        let function = stripped_empty_switches.as_ref().unwrap_or(function);
        let structured_switch_source = function.clone();
        let repeated_call_poll_transaction = is_repeated_call_poll_transaction(function);
        let lowered_switches = lower_structured_switches(function);
        let function = lowered_switches.as_ref().unwrap_or(function);
        let retains_unobserved_local_lane = function
            .locals
            .iter()
            .any(|local| is_unobserved_local_assignment(function, &local.name));
        let raw_loop_assertion_strings = plan_loop_assertion_strings(function);
        let planned_loop_assertion_strings = raw_loop_assertion_strings.filter(|plan| {
            self.behavior.frame_convention == FrameConvention::Predecrement
                && self.behavior.global_addressing == GlobalAddressing::SmallData
                && self.variadic_callees.contains(&plan.callee)
        });
        let capture = std::env::var_os("MWCC_CAPTURE_FUNCTION")
            .is_some_and(|name| name == std::ffi::OsStr::new(&function.name));
        macro_rules! decline {
            ($reason:expr) => {{
                if capture {
                    eprintln!(
                        "structured body declined (frame_mode={with_frame_array}): {}",
                        $reason
                    );
                }
                return Ok(false);
            }};
        }
        if !function.guards.is_empty()
            || self.frame_slots.values().any(|slot| {
                slot.is_array
                    || slot.parameter_register.is_some()
                    || !matches!(slot.value_type, Type::Struct { .. })
            })
        {
            decline!(format!(
                "pre-existing frame slots={}, guards={}",
                self.frame_slots.len(),
                function.guards.len()
            ));
        }
        let aggregate_frame_locals: Vec<_> = if with_frame_array {
            function
                .locals
                .iter()
                .filter(|local| {
                    matches!(local.declared_type, Type::Struct { .. })
                        && !self.one_word_aggregate_locals.contains(&local.name)
                        && body_uses_local(&function.statements, &local.name)
                })
                .collect()
        } else {
            Vec::new()
        };
        let address_taken = crate::frame::collect_address_taken(function);
        let frame_scalar_parameters: Vec<_> = if with_frame_array {
            function
                .parameters
                .iter()
                .filter(|parameter| address_taken.contains(parameter.name.as_str()))
                .collect()
        } else {
            Vec::new()
        };
        let frame_scalar_locals: Vec<&LocalDeclaration> = if with_frame_array {
            function
                .locals
                .iter()
                .filter(|local| {
                    local.array_length.is_none()
                        && !matches!(local.declared_type, Type::Struct { .. })
                        && address_taken.contains(local.name.as_str())
                })
                .collect()
        } else {
            Vec::new()
        };
        if frame_scalar_parameters.iter().any(|parameter| {
            class_of(parameter.parameter_type).ok() != Some(ValueClass::General)
                || parameter.parameter_type.width() > 32
        }) || frame_scalar_locals.iter().any(|local| {
            local.is_static
                || !matches!(
                    class_of(local.declared_type),
                    Ok(ValueClass::General | ValueClass::Float)
                )
                || local.declared_type.width() > 32
                || local
                    .initializer
                    .as_ref()
                    .is_some_and(crate::analysis::expression_has_call)
        }) {
            decline!("an address-taken scalar cannot use the structured frame");
        }
        let int_to_float_conversion_count =
            self.count_integer_to_float_conversions(function);
        let float_to_int_conversion_count =
            self.count_float_to_integer_conversions(function);
        let frame_array_plan = if with_frame_array {
            let Some(plan) = plan_structured_frame_arrays(function) else {
                decline!("automatic array shape is unsupported");
            };
            if plan.arrays.is_empty()
                && aggregate_frame_locals.is_empty()
                && frame_scalar_parameters.is_empty()
                && frame_scalar_locals.is_empty()
                && int_to_float_conversion_count == 0
                && float_to_int_conversion_count == 0
            {
                decline!(
                    "frame mode requires an automatic array, aggregate, scalar, or conversion slot"
                );
            }
            if !((function.return_type == Type::Void && function.return_expression.is_none())
                    || (matches!(
                        function.return_type,
                        Type::Int
                            | Type::UnsignedInt
                            | Type::Short
                            | Type::UnsignedShort
                            | Type::Char
                            | Type::UnsignedChar
                            | Type::Pointer(_)
                            | Type::StructPointer { .. }
                    )
                        && function.return_expression.is_some()))
            {
                decline!("automatic-array return shape is unsupported");
            }
            plan
        } else {
            super::structured_frame_arrays::StructuredFrameArrays {
                arrays: Vec::new(),
                image_sources: Vec::new(),
                total_bytes: 0,
            }
        };
        let frame_arrays = &frame_array_plan.arrays;
        let frame_array_image_sources = &frame_array_plan.image_sources;
        let frame_array_bytes = frame_array_plan.total_bytes;
        let array_pool_plan = (self.behavior.frame_convention == FrameConvention::Predecrement)
            .then(|| plan_structured_array_pool(frame_arrays, frame_array_image_sources))
            .flatten();
        let global_index_cache_plan = array_pool_plan.as_ref().and_then(|_| {
            plan_structured_global_index_cache(
                function,
                &self.globals,
                &self.global_array_sizes,
            )
        });
        let global_base_cache_plan =
            plan_structured_global_base_cache(function, &self.global_array_sizes);
        let global_member_address_cache_plan = plan_structured_global_member_address_cache(
            function,
            &self.addressable_globals,
            &self.global_array_sizes,
        );
        let aggregate_call_copy_plan =
            (frame_arrays.is_empty()
                && frame_scalar_parameters.is_empty()
                && frame_scalar_locals.is_empty())
                .then(|| {
                    plan_terminal_one_word_aggregate_call_copies(
                        &aggregate_frame_locals,
                        &function.locals,
                        &function.statements,
                        &self.call_parameter_types,
                    )
                })
                .flatten();
        let aggregate_call_copy_bytes = aggregate_call_copy_plan
            .as_ref()
            .map_or(0, |plan| plan.total_bytes);
        let unused_frame_array = !frame_arrays.is_empty()
            && frame_arrays
                .iter()
                .all(|array| !body_uses_local(&function.statements, &array.name));
        let interleaved_frame_layout = StructuredInterleavedFrameLayout::plan(
            function,
            frame_arrays,
            &aggregate_frame_locals,
            unused_frame_array,
            self.behavior.frame_convention,
        );
        if capture {
            eprintln!(
                "structured interleaved frame layout: {} frame={:?} arrays={:?} aggregates={:?}",
                interleaved_frame_layout.is_some(),
                self.behavior.frame_convention,
                frame_arrays
                    .iter()
                    .map(|local| (
                        local.name.as_str(),
                        local.declared_type,
                        local.array_length,
                    ))
                    .collect::<Vec<_>>(),
                aggregate_frame_locals
                    .iter()
                    .map(|local| local.name.as_str())
                    .collect::<Vec<_>>(),
            );
        }
        // Keep source loops visible to definite-assignment and lifetime
        // planning. Their canonical label/goto graph is only the emission view.
        let lowered_structured_function =
            lower_structured_loops(function, &self.global_array_sizes);
        let structured_function = lowered_structured_function.as_ref().unwrap_or(function);
        let emission_switches =
            lower_structured_switches_for_emission(&structured_switch_source);
        let emission_switch_function = emission_switches
            .as_ref()
            .unwrap_or(&structured_switch_source);
        let lowered_emission_function =
            lower_structured_loops(emission_switch_function, &self.global_array_sizes);
        let emission_function = lowered_emission_function
            .as_ref()
            .unwrap_or(emission_switch_function);
        let supported_plain_return = structured_return_is_supported(function);
        if (!with_frame_array && !supported_plain_return)
            || !supports_statements(
                &structured_function.statements,
                function,
                &self.global_array_sizes,
                with_frame_array,
            )
        {
            decline!("statement or return shape is unsupported");
        }

        let candidates: Vec<&str> = function
            .locals
            .iter()
            .filter(|local| {
                local.array_length.is_none() && !address_taken.contains(local.name.as_str())
            })
            .map(|local| local.name.as_str())
            .chain(
                function
                    .parameters
                    .iter()
                    .filter(|parameter| {
                        !address_taken.contains(parameter.name.as_str())
                    })
                    .map(|parameter| parameter.name.as_str()),
            )
            .collect();
        let mut survivors: std::collections::HashSet<&str> = candidates
            .into_iter()
            .filter(|name| {
                read_after_possible_call_in_function(function, name)
                    || self.inline_source_call_survivors.contains(*name)
                    || (self.one_word_aggregate_locals.contains(*name)
                    && body_uses_local(&function.statements, name)
                    && function.statements.iter().any(statement_has_call))
            })
            .collect();
        let call_accumulators = call_accumulator_names(function);
        // Entry-initialized locals rank ahead of incoming parameters. Deferred
        // locals introduced by nested declarations or inline expansion rank
        // after them and may share a home when their lifetimes do not overlap.
        let mut saved_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| {
                local.array_length.is_none()
                    && survivors.contains(local.name.as_str())
                    && !call_accumulators.contains(local.name.as_str())
                    && (self.one_word_aggregate_locals.contains(&local.name)
                        || !is_transient_direct_call_argument_local(
                            &function.statements,
                            function.return_expression.as_ref(),
                            &local.name,
                        ))
            })
            .collect();
        let mut saved_parameters: Vec<_> = function
            .parameters
            .iter()
            .rev()
            .filter(|parameter| survivors.contains(parameter.name.as_str()))
            .collect();
        // A parameter returned after the call graph owns the longest visible
        // lifetime. MWCC gives it the highest callee-saved home even when a
        // later parameter is also live to the final call (notably `this` in a
        // complete-object deleting destructor).
        if let Some(Expression::Variable(returned)) = function.return_expression.as_ref() {
            if let Some(index) = saved_parameters
                .iter()
                .position(|parameter| parameter.name == *returned)
            {
                saved_parameters[..=index].rotate_right(1);
            }
        }
        if saved_parameters.iter().any(|parameter| {
            self.locations.get(&parameter.name).is_none_or(|location| {
                !matches!(location.class, ValueClass::General | ValueClass::Float)
            })
        }) {
            decline!("a saved parameter is neither a general nor floating value");
        }
        let (saved_float_parameters, mut saved_parameters): (Vec<_>, Vec<_>) =
            saved_parameters.into_iter().partition(|parameter| {
                self.locations
                    .get(&parameter.name)
                    .is_some_and(|location| location.class == ValueClass::Float)
            });
        let promoted_indirect_call_locals = promote_cost_free_indirect_call_locals(
            function,
            &survivors,
            &saved_parameters,
            &saved_locals,
        );
        if !promoted_indirect_call_locals.is_empty() {
            survivors.extend(
                promoted_indirect_call_locals
                    .iter()
                    .map(|local| local.name.as_str()),
            );
            saved_locals = function
                .locals
                .iter()
                .filter(|local| {
                    local.array_length.is_none()
                        && survivors.contains(local.name.as_str())
                        && !call_accumulators.contains(local.name.as_str())
                        && (self.one_word_aggregate_locals.contains(&local.name)
                            || !is_transient_direct_call_argument_local(
                                &function.statements,
                                function.return_expression.as_ref(),
                                &local.name,
                            ))
                })
                .collect();
        }
        let source_saved_local_count = saved_locals
            .iter()
            .filter(|local| self.inline_source_call_survivors.contains(&local.name))
            .count();
        if source_saved_local_count < 2 {
            self.inline_source_call_survivors.clear();
        } else {
            // The original guarded value diamond owns one eliminated
            // optimizer binding lane even though late semantic composition
            // leaves no ordinary statement-body residue.
            self.legacy_inline_expansion_frame_bytes =
                self.legacy_inline_expansion_frame_bytes.max(8);
        }
        if capture {
            let mut survivor_names: Vec<_> = survivors.iter().copied().collect();
            survivor_names.sort_unstable();
            eprintln!(
                "structured body plan: survivors={survivor_names:?} saved_locals={:?}",
                saved_locals
                    .iter()
                    .map(|local| local.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
        if saved_locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || !matches!(
                    class_of(local.declared_type),
                    Ok(ValueClass::General | ValueClass::Float)
                )
        }) {
            decline!(format!(
                "a saved local is unsupported: {}",
                saved_locals
                    .iter()
                    .filter(|local| {
                        local.is_static
                            || local.array_length.is_some()
                            || !matches!(
                                class_of(local.declared_type),
                                Ok(ValueClass::General | ValueClass::Float)
                            )
                    })
                    .map(|local| format!("{}:{:?}", local.name, local.declared_type))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let Some(ephemeral_locals) = plan_ephemeral_locals(function, &survivors, &address_taken)
        else {
            decline!("ephemeral-local planning rejected the body");
        };
        let dense_loop_window =
            plan_dense_loop_register_window(&function.statements, &ephemeral_locals);
        let dense_loop_carried =
            plan_dense_loop_carried_locals(&function.statements, &ephemeral_locals);
        let frame_publication =
            StructuredFramePublication::plan(function, &frame_scalar_locals, dense_loop_window);
        if let Some(publication) = &frame_publication {
            saved_parameters.retain(|parameter| parameter.name != publication.parameter);
        }
        // Frame provenance belongs to the source CFG. Lowering a switch into
        // nested `if` statements is an emission detail and must not make an
        // exhaustively selected result look like a source guarded local.
        let retained_deferred_local_lane = retains_deferred_saved_local_lane(
            &structured_switch_source.statements,
            &saved_locals,
        );
        let (saved_float_locals, saved_locals): (Vec<_>, Vec<_>) = saved_locals
            .into_iter()
            .partition(|local| class_of(local.declared_type).ok() == Some(ValueClass::Float));
        let Some(saved_float_plan) = plan_deferred_saved_homes(function, &saved_float_locals)
        else {
            decline!("saved float-home planning rejected the body");
        };
        let saved_float_count = saved_float_parameters
            .len()
            .checked_add(saved_float_plan.group_count)
            .ok_or_else(|| Diagnostic::error("saved float-home count overflow"))?;
        if saved_float_count > 18 {
            decline!("more than eighteen overlapping saved float values are live");
        }
        let (eager_saved_locals, deferred_saved_locals): (Vec<_>, Vec<_>) = saved_locals
            .into_iter()
            .partition(|local| local.initializer.is_some());
        let precomposition_home_layout = StructuredPrecompositionHomeLayout::plan(
            &deferred_saved_locals,
            &self.inline_source_call_survivors,
        );
        let complement_product_pair = StructuredComplementProductPair::plan(
            function,
            &saved_float_locals,
            &eager_saved_locals,
            self.behavior.frame_convention,
        );
        let saved_parameter_names = saved_parameters
            .iter()
            .chain(saved_float_parameters.iter())
            .map(|parameter| parameter.name.as_str())
            .collect();
        let initializer_live_in = self.plan_initializer_live_in(
            function,
            &eager_saved_locals,
            &saved_parameter_names,
        );
        let deferred_home_plan = if self.inline_source_call_survivors.is_empty() {
            plan_deferred_saved_homes(function, &deferred_saved_locals)
        } else {
            plan_distinct_deferred_saved_homes(function, &deferred_saved_locals)
        };
        let Some(deferred_home_plan) = deferred_home_plan else {
            decline!("deferred saved-home planning rejected the body");
        };
        let unused_array_state_transfer = unused_frame_array
            && is_unused_array_state_transfer(function)
            && eager_saved_locals.len() == 3
            && saved_parameters.len() == 2
            && deferred_home_plan.group_count == 0;
        let path_reuse_frame_bytes = i16::try_from(4 * deferred_home_plan.path_reuse_count)
            .map_err(|_| Diagnostic::error("structured path-reuse frame is too large"))?;
        let variadic_output_frame = (self.behavior.frame_convention
            == FrameConvention::LinkageFirst)
            .then(|| {
                StructuredVariadicOutputFrame::plan(
                    function,
                    frame_arrays,
                    frame_array_bytes,
                    &frame_scalar_locals,
                    frame_scalar_parameters.len(),
                    aggregate_frame_locals.len(),
                    int_to_float_conversion_count,
                    &self.variadic_callees,
                )
            })
            .flatten();
        let linkage_first_scalar_local_table_bytes = if frame_arrays.is_empty()
            && frame_scalar_parameters.is_empty()
            && frame_publication.is_none()
            && self.behavior.frame_convention == FrameConvention::LinkageFirst
        {
            i16::try_from(frame_scalar_locals.len() * 4)
                .map_err(|_| Diagnostic::error("structured scalar frame is too large"))?
        } else {
            0
        };
        let scalar_only_frame_bytes = if frame_arrays.is_empty() {
            frame_publication.as_ref().map_or_else(
                || {
                    i16::try_from(
                        (frame_scalar_parameters.len() + frame_scalar_locals.len()) * 4,
                    )
                },
                |_| Ok(LOCAL_REGION_BYTES),
            )
                .map_err(|_| Diagnostic::error("structured scalar frame is too large"))?
                .checked_add(linkage_first_scalar_local_table_bytes)
                .ok_or_else(|| Diagnostic::error("structured scalar frame is too large"))?
        } else {
            0
        };
        let mut local_region_bytes = if let Some(layout) = &interleaved_frame_layout {
            layout.local_region_bytes()
        } else if !aggregate_frame_locals.is_empty() {
            let mut end = 8u32
                .checked_add(u32::try_from(aggregate_call_copy_bytes).map_err(|_| {
                    Diagnostic::error("structured aggregate copy area is out of range")
                })?)
                .ok_or_else(|| Diagnostic::error("structured aggregate frame is too large"))?;
            for local in aggregate_frame_locals.iter().rev() {
                let Type::Struct { size, align } = local.declared_type else {
                    unreachable!("aggregate frame locals were filtered")
                };
                let align = u32::from(align.max(1));
                end = end.div_ceil(align) * align;
                end = end
                    .checked_add(size)
                    .ok_or_else(|| Diagnostic::error("structured aggregate frame is too large"))?;
            }
            i16::try_from(end.saturating_sub(8))
                .map_err(|_| Diagnostic::error("structured aggregate frame is too large"))?
                .checked_add(frame_array_bytes)
                .and_then(|bytes| bytes.checked_add(scalar_only_frame_bytes))
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?
        } else if !self.frame_slots.is_empty() {
            let end = self
                .frame_slots
                .values()
                .map(|slot| i32::from(slot.offset) + i32::try_from(slot.size).unwrap_or(i32::MAX))
                .max()
                .unwrap_or(8);
            i16::try_from(end.saturating_sub(8))
                .map_err(|_| Diagnostic::error("structured aggregate frame is too large"))?
        } else if !frame_arrays.is_empty() {
            if unused_array_state_transfer {
                0
            } else {
                frame_array_bytes
            }
        } else {
            scalar_only_frame_bytes
        };
        if let Some(frame) = &variadic_output_frame {
            local_region_bytes = frame
                .local_end
                .checked_sub(8)
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
        }
        if float_to_int_conversion_count != 0 {
            let occupied_end = 8i16
                .checked_add(local_region_bytes)
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
            let conversion_base = occupied_end
                .checked_add(7)
                .map(|end| end & !7)
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
            self.plan_float_to_int_scratch(conversion_base, float_to_int_conversion_count)?;
            local_region_bytes = self
                .float_to_int_scratch_end
                .checked_sub(8)
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
        }
        if int_to_float_conversion_count != 0 {
            let conversion_base = if let Some(frame) = &variadic_output_frame {
                frame.conversion_base
            } else {
                let occupied_end = 8i16
                    .checked_add(local_region_bytes)
                    .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
                occupied_end
                    .checked_add(7)
                    .map(|end| end & !7)
                    .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?
            };
            self.plan_int_to_float_scratch(
                conversion_base,
                int_to_float_conversion_count,
            )?;
            local_region_bytes = self
                .int_to_float_scratch_end
                .checked_sub(8)
                .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
        }
        let global_member_search_entry = function.statements.first().is_some_and(|statement| {
            super::super::global_struct_member_search::is_global_struct_member_search_loop(
                statement,
                &self.global_array_sizes,
            )
        });
        let rounded_byte_pointer = global_member_search_entry
            .then(|| adjacent_byte_pointer_round_up_name(function))
            .flatten();
        let folded_terminal_pointer_alias = function
            .statements
            .iter()
            .enumerate()
            .any(|(index, _)| is_folded_terminal_pointer_load_alias(function, index));

        let eager_home_reuse =
            StructuredEagerHomeReuse::plan(function, &eager_saved_locals, &deferred_home_plan);
        let parameter_home_reuse = StructuredParameterHomeReuse::plan(
            function,
            eager_saved_locals.len(),
            &saved_parameters,
            &deferred_home_plan,
            &eager_home_reuse,
        );
        let returned_deferred_home = function
            .return_expression
            .as_ref()
            .and_then(|expression| match expression {
                Expression::Variable(name) => deferred_saved_locals
                    .iter()
                    .find(|local| local.name == *name),
                _ => None,
            })
            .map(|local| {
                parameter_home_reuse.home_index(deferred_home_plan.group(&local.name))
            });
        let returned_deferred_parameter_home = returned_deferred_home.is_some_and(|home| {
            (eager_saved_locals.len()
                ..eager_saved_locals.len() + saved_parameters.len())
                .contains(&home)
        });
        let value_home_count = eager_saved_locals.len()
            + saved_parameters.len()
            + parameter_home_reuse.fresh_group_count;
        let loop_assertion_strings =
            (value_home_count == 4).then_some(planned_loop_assertion_strings).flatten();
        let base_home_count = value_home_count + 2 * usize::from(loop_assertion_strings.is_some());
        let count = dense_loop_window
            .filter(|window| value_home_count <= *window)
            .unwrap_or(base_home_count);
        let loop_member_receiver_layout = StructuredLoopMemberReceiverLayout::plan(
            function,
            &eager_saved_locals,
            &saved_parameters,
            &deferred_saved_locals,
            &deferred_home_plan,
            &parameter_home_reuse,
            count,
        );
        let object_collision_loop_layout =
            StructuredObjectCollisionLoopLayout::plan(
                function,
                &eager_saved_locals,
                &saved_parameters,
                &deferred_saved_locals,
                &deferred_home_plan,
                &parameter_home_reuse,
                count,
            );
        if capture {
            eprintln!(
                "structured object collision loop layout: {} \
                 (eager={}, parameters={}, deferred={}, groups={}, fresh={}, homes={})",
                object_collision_loop_layout.is_some(),
                eager_saved_locals.len(),
                saved_parameters.len(),
                deferred_saved_locals.len(),
                deferred_home_plan.group_count,
                parameter_home_reuse.fresh_group_count,
                count,
            );
        }
        let returned_deferred_pair = returned_deferred_pair_preference(
            with_frame_array,
            eager_saved_locals.len(),
            saved_parameters.len(),
            deferred_home_plan.group_count,
            count,
            returned_deferred_home,
            0,
        )
        .is_some();
        if returned_deferred_pair || returned_deferred_parameter_home {
            self.epilogue_lr_before_gprs = true;
        }
        let unused_array_two_homes = unused_frame_array
            && saved_parameters.is_empty()
            && count == 2
            && deferred_home_plan.group_count != 0;
        let unused_array_eager_homes = unused_frame_array
            && saved_parameters.is_empty()
            && deferred_home_plan.group_count == 0
            && eager_saved_locals.len() >= 2;
        let unused_array_aggregate_eager_homes = unused_array_eager_homes
            && frame_array_bytes == 4
            && !aggregate_frame_locals.is_empty();
        let compact_aggregate_scratch_pair = compact_aggregate_scratch_frame_pair(
            unused_frame_array,
            frame_array_bytes,
            aggregate_frame_locals.len(),
            eager_saved_locals.len(),
            saved_parameters.len(),
            deferred_home_plan.group_count,
            count,
        );
        let reused_data_anchor_home_index = self
            .data_section_anchor
            .as_ref()
            .filter(|_| array_pool_plan.is_none())
            .and_then(|anchor| {
                super::linkage_first_data_anchor::reusable_deferred_group(
                    function,
                    anchor,
                    &deferred_home_plan,
                )
            })
            .map(|group| parameter_home_reuse.home_index(group))
            .filter(|home_index| {
                *home_index >= eager_saved_locals.len() + saved_parameters.len()
            });
        let has_standalone_data_anchor = self.data_section_anchor.is_some()
            && array_pool_plan.is_none()
            && reused_data_anchor_home_index.is_none();
        let exclusive_arm_home_layout = ExclusiveArmHomeLayout::plan(
            with_frame_array,
            has_standalone_data_anchor,
            eager_saved_locals.len(),
            saved_parameters.len(),
            count,
            &deferred_home_plan,
            &parameter_home_reuse,
        );
        let standalone_data_anchor_home = has_standalone_data_anchor.then(|| {
            self.fresh_virtual_general_preferring(
                exclusive_arm_home_layout
                    .as_ref()
                    .map_or(31, ExclusiveArmHomeLayout::data_anchor_preference),
            )
        });
        let standalone_global_member_address_home =
            global_member_address_cache_plan.as_ref().map(|_| {
                self.fresh_virtual_general_preferring(if standalone_data_anchor_home.is_some() {
                    30
                } else {
                    31
                })
            });
        let saved_home_slot_base = usize::from(standalone_data_anchor_home.is_some())
            + usize::from(standalone_global_member_address_home.is_some());
        let total_home_count = count + saved_home_slot_base;
        let first_saved = 32usize.saturating_sub(total_home_count);
        let frame_first_saved = array_pool_plan
            .as_ref()
            .map_or(first_saved, |plan| {
                first_saved.min(usize::from(plan.first_saved_register))
            });
        let frame_saved_count = 32usize.saturating_sub(frame_first_saved);
        let loop_assertion_saved_range = loop_assertion_strings.is_some();
        let dense_unused_array_state_transfer =
            unused_array_state_transfer && count == 5;
        let dense_frame = uses_dense_saved_register_range(
            with_frame_array,
            eager_saved_locals.len(),
            count,
            global_member_search_entry,
            parameter_home_reuse
                .reuses_parameter_home(eager_saved_locals.len(), saved_parameters.len()),
        ) || dense_unused_array_state_transfer;
        let dense_retained_local_table = (dense_frame
            && self.behavior.frame_convention == FrameConvention::LinkageFirst
            && !frame_arrays.is_empty()
            && !frame_scalar_locals.is_empty()
            && self.legacy_inline_expansion_frame_bytes != 0)
            .then(|| UnobservedScalarTable::plan(function))
            .flatten();
        let dense_retained_local_table_bytes =
            if let Some(table) = dense_retained_local_table {
                table
                    .bytes
                    .checked_add(
                        i16::try_from(self.legacy_inline_expansion_frame_bytes).map_err(|_| {
                            Diagnostic::error("structured inline frame table is too large")
                        })?,
                    )
                    .ok_or_else(|| {
                        Diagnostic::error("structured retained local table is too large")
                    })?
            } else {
                0
            };
        // A source loop may retain assertion string high halves alongside its
        // values in one contiguous saved-GPR range without otherwise using
        // dense-frame entry or body scheduling.
        let dense_saved_range =
            dense_frame || loop_assertion_saved_range || array_pool_plan.is_some();
        let dense_eager_round_up = dense_frame
            .then(|| plan_dense_eager_pointer_round_up(function))
            .flatten();
        let dense_entry_prefix = with_frame_array
            && !dense_unused_array_state_transfer
            && !global_member_search_entry
            && structured_dense_frame_entry_index(function).is_some_and(|index| index != 0);
        let search_result = function.statements.first().and_then(|statement| {
            super::super::global_struct_member_search::global_struct_member_search_result(statement)
        });
        let search_result_is_keystone = search_result.is_some_and(|name| {
            function
                .statements
                .iter()
                .skip(1)
                .filter(|statement| statement_references_name(statement, name))
                .count()
                >= 6
        });
        let mut global_group_order = Vec::new();
        if global_member_search_entry {
            if search_result_is_keystone {
                if let Some(result) = search_result {
                    if let Some(local) = deferred_saved_locals
                        .iter()
                        .find(|local| local.name == result)
                    {
                        global_group_order.push(deferred_home_plan.group(&local.name));
                    }
                }
            }
            for local in &function.locals {
                if deferred_saved_locals
                    .iter()
                    .any(|saved| saved.name == local.name)
                {
                    let group = deferred_home_plan.group(&local.name);
                    if !global_group_order.contains(&group) {
                        global_group_order.push(group);
                    }
                }
            }
        }
        let deferred_preference_base = eager_saved_locals.len() + saved_parameters.len();
        let rounded_pointer_lifetime_order = dense_eager_round_up.is_some()
            && uses_rounded_pointer_dense_layout(
                eager_saved_locals.len(),
                saved_parameters.len(),
                count,
            );
        let rounded_pointer_dense_layout = rounded_pointer_lifetime_order
            && self.behavior.power_pc_7400_scheduling_enabled();
        let dense_deferred_preferences = dense_eager_deferred_preferences(
            eager_saved_locals.len(),
            saved_parameters.len(),
            count,
            &deferred_home_plan,
            &parameter_home_reuse,
            rounded_pointer_dense_layout,
            rounded_pointer_lifetime_order,
        );
        let allocator_cursor_preferences =
            (dense_frame && self.behavior.frame_convention == FrameConvention::Predecrement)
                .then(|| {
                    allocator_result_cursor_preferences(
                        function,
                        &deferred_home_plan,
                        eager_saved_locals.len(),
                        saved_parameters.len(),
                        count,
                    )
                })
                .unwrap_or_default();
        let async_callback_switch_layout = StructuredAsyncCallbackSwitchLayout::plan(
            &structured_switch_source,
            with_frame_array,
            eager_saved_locals.len(),
            &saved_parameters,
            &deferred_saved_locals,
            &deferred_home_plan,
            &parameter_home_reuse,
            count,
        );
        let retained_store_constant_homes = eager_saved_locals.is_empty()
            && saved_parameters.is_empty()
            && count == deferred_home_plan.group_count
            && deferred_saved_locals.len() >= 2
            && deferred_saved_locals
                .iter()
                .all(|local| local.name.starts_with("__mwcc_retained_constant_"));
        let sequenced_callback_wait_layout = is_sequenced_callback_wait_layout(
            function,
            &saved_parameters,
            &deferred_saved_locals,
            first_saved,
        );
        if sequenced_callback_wait_layout {
            self.structured_sequenced_callback_wait_starter =
                sequenced_callback_wait_starter(function).map(str::to_owned);
            self.structured_cfg_cleanup_owner = true;
        }
        let homes: Vec<u8> = (0..count)
            .map(|home_index| {
                if loop_assertion_strings.is_some() {
                    let preferred = match home_index {
                        0 => 26,
                        1 => 27,
                        2 => 31,
                        3 => 30,
                        4 => 28,
                        5 => 29,
                        _ => unreachable!("loop assertion plan has six saved homes"),
                    };
                    self.fresh_virtual_general_preferring(preferred)
                } else if retained_store_constant_homes {
                    self.fresh_virtual_general_preferring((first_saved + home_index) as u8)
                } else if let Some(preferred) = loop_member_receiver_layout
                    .as_ref()
                    .and_then(|layout| layout.preference(home_index))
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = object_collision_loop_layout
                    .as_ref()
                    .and_then(|layout| layout.preference(home_index))
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = frame_publication
                    .as_ref()
                    .and_then(|publication| {
                        (home_index >= eager_saved_locals.len()
                            && home_index
                                < eager_saved_locals.len() + saved_parameters.len())
                        .then(|| {
                            publication.saved_parameter_preference(
                                home_index - eager_saved_locals.len(),
                            )
                        })
                        .flatten()
                    })
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(&preferred) = allocator_cursor_preferences.get(&home_index) {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = async_callback_switch_layout
                    .as_ref()
                    .and_then(|layout| layout.preference(home_index))
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = variadic_output_frame
                    .as_ref()
                    .and_then(|frame| {
                        frame.saved_home_preference(
                            eager_saved_locals.len(),
                            saved_parameters.len(),
                            count,
                            home_index,
                        )
                    })
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = complement_product_pair
                    .as_ref()
                    .and_then(|pair| pair.saved_general_home_preference(count, home_index))
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = exclusive_arm_home_layout
                    .as_ref()
                    .and_then(|layout| layout.preference(home_index))
                {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = paired_eager_deferred_preference(
                    with_frame_array,
                    eager_saved_locals.len(),
                    saved_parameters.len(),
                    deferred_home_plan.group_count,
                    self.legacy_inline_expansion_frame_bytes != 0,
                    home_index,
                ) {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = returned_deferred_pair_preference(
                    with_frame_array,
                    eager_saved_locals.len(),
                    saved_parameters.len(),
                    deferred_home_plan.group_count,
                    count,
                    returned_deferred_home,
                    home_index,
                ) {
                    self.fresh_virtual_general_preferring(preferred)
                } else if rounded_pointer_dense_layout {
                    let preferred = dense_deferred_preferences
                        .get(&home_index)
                        .copied()
                        .or_else(|| {
                            rounded_pointer_dense_home_preference(
                                eager_saved_locals.len(),
                                saved_parameters.len(),
                                count,
                                home_index,
                            )
                        });
                    if let Some(register) = preferred {
                        self.fresh_virtual_general_preferring(register)
                    } else {
                        self.fresh_virtual_general()
                    }
                } else if dense_unused_array_state_transfer {
                    self.fresh_virtual_general_preferring(
                        31u8.saturating_sub(u8::try_from(home_index).unwrap_or(4).min(4)),
                    )
                } else if dense_frame && !eager_saved_locals.is_empty() {
                    let preferred = dense_deferred_preferences
                        .get(&home_index)
                        .copied()
                        .or_else(|| {
                            dense_eager_home_preference(
                                eager_saved_locals.len(),
                                saved_parameters.len(),
                                count,
                                home_index,
                            )
                        });
                    if let Some(register) = preferred {
                        self.fresh_virtual_general_preferring(register)
                    } else {
                        self.fresh_virtual_general()
                    }
                } else if global_member_search_entry && home_index >= deferred_preference_base {
                    let group = home_index - deferred_preference_base;
                    let rank = global_group_order
                        .iter()
                        .position(|candidate| *candidate == group)
                        .unwrap_or(group);
                    self.fresh_virtual_general_preferring(31u8.saturating_sub(rank as u8))
                } else if unused_array_two_homes || unused_array_aggregate_eager_homes {
                    // A dead scratch array keeps its source frame bytes but no
                    // value node. The retained values keep source creation
                    // order from the bottom of the saved-register range.
                    self.fresh_virtual_general_preferring((first_saved + home_index) as u8)
                } else if let Some(preferred) = direct_callback_wait_home_preference(
                    function,
                    &saved_parameters,
                    &deferred_saved_locals,
                    first_saved,
                    home_index,
                ) {
                    self.fresh_virtual_general_preferring(preferred)
                } else if let Some(preferred) = sequenced_callback_wait_home_preference(
                    function,
                    &saved_parameters,
                    &deferred_saved_locals,
                    first_saved,
                    home_index,
                ) {
                    self.fresh_virtual_general_preferring(preferred)
                } else if with_frame_array && eager_saved_locals.is_empty() && count <= 18 {
                    let preferred = if dense_entry_prefix && deferred_home_plan.group_count == 1 {
                        if home_index < saved_parameters.len() {
                            let source_index = saved_parameters.len() - 1 - home_index;
                            first_saved + (source_index + 2) % count
                        } else {
                            first_saved + 1
                        }
                    } else if home_index < saved_parameters.len() {
                        first_saved + saved_parameters.len() - 1 - home_index
                    } else {
                        first_saved + home_index
                    };
                    self.fresh_virtual_general_preferring(preferred as u8)
                } else {
                    self.fresh_virtual_general()
                }
            })
            .collect();
        if let Some(layout) = &precomposition_home_layout {
            for local in &deferred_saved_locals {
                let Some(preferred) = layout.preference(&local.name) else {
                    continue;
                };
                let group = deferred_home_plan.group(&local.name);
                let home = homes[parameter_home_reuse.home_index(group)];
                self.prefer_virtual_general(home, preferred);
            }
        }
        let data_section_anchor_home = reused_data_anchor_home_index
            .map(|home_index| homes[home_index])
            .or(standalone_data_anchor_home);
        self.data_section_anchor_reuses_deferred_home =
            reused_data_anchor_home_index.is_some();
        if let Some(register) = data_section_anchor_home {
            self.data_section_anchor
                .as_mut()
                .expect("the data anchor was planned above")
                .register = Some(register);
        }
        let frame_slot_for_home = |home_index: usize| {
            if let Some(layout) = &loop_member_receiver_layout {
                layout.frame_slot(home_index)
            } else if sequenced_callback_wait_layout {
                sequenced_callback_wait_frame_slot(home_index)
                    .expect("the sequenced callback wait layout owns three homes")
            } else {
                match reused_data_anchor_home_index {
                    Some(reused) if home_index == reused => 0,
                    Some(reused) if home_index < reused => home_index + 1,
                    _ => saved_home_slot_base + home_index,
                }
            }
        };
        let reused_data_anchor_slot = reused_data_anchor_home_index.map(frame_slot_for_home);
        if let Some(strings) = &loop_assertion_strings {
            self.loop_assertion_string_highs = vec![
                (strings.file.clone(), homes[value_home_count]),
                (strings.asserted.clone(), homes[value_home_count + 1]),
            ];
        }
        let mut logical_saved_homes = Vec::with_capacity(total_home_count);
        logical_saved_homes.extend(standalone_data_anchor_home);
        logical_saved_homes.extend(standalone_global_member_address_home);
        if let Some(reused) = reused_data_anchor_home_index {
            logical_saved_homes.push(homes[reused]);
            logical_saved_homes.extend(
                homes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, home)| (index != reused).then_some(*home)),
            );
        } else if let Some(layout) = &precomposition_home_layout {
            logical_saved_homes.extend(layout.save_order().map(|name| {
                let group = deferred_home_plan.group(name);
                homes[parameter_home_reuse.home_index(group)]
            }));
        } else if let Some(layout) = &loop_member_receiver_layout {
            logical_saved_homes.extend(
                layout
                    .save_order()
                    .into_iter()
                    .map(|home_index| homes[home_index]),
            );
        } else if sequenced_callback_wait_layout {
            let [identifier, receiver, wait_state] = homes.as_slice() else {
                unreachable!("the sequenced callback wait layout owns three homes")
            };
            logical_saved_homes.extend([*wait_state, *identifier, *receiver]);
        } else if let Some(layout) = &async_callback_switch_layout {
            logical_saved_homes.extend(layout.save_order(&homes));
        } else {
            logical_saved_homes.extend(homes.iter().copied());
        }
        let mut frame_homes = logical_saved_homes.clone();
        frame_homes.resize(frame_saved_count, frame_first_saved as u8);
        let mut plan = mwcc_vreg::FramePlan::with_local_region(frame_homes, local_region_bytes);
        plan.frame_size = plan
            .frame_size
            .checked_add(path_reuse_frame_bytes)
            .ok_or_else(|| Diagnostic::error("structured path-reuse frame is too large"))?;
        if aggregate_call_copy_plan.is_some()
            && self.behavior.frame_convention == FrameConvention::LinkageFirst
            && homes.is_empty()
        {
            plan.frame_size = 8i16
                .checked_add(local_region_bytes)
                .and_then(|size| size.checked_add(path_reuse_frame_bytes))
                .ok_or_else(|| Diagnostic::error("structured aggregate frame is too large"))?;
        }
        if !aggregate_frame_locals.is_empty() {
            let placements = if aggregate_call_copy_bytes == 0 {
                plan_aggregate_frame_slots(&aggregate_frame_locals, &function.statements)?
            } else {
                plan_aggregate_frame_slots_from(
                    &aggregate_frame_locals,
                    &function.statements,
                    8 + u32::try_from(aggregate_call_copy_bytes).map_err(|_| {
                        Diagnostic::error("structured aggregate copy area is out of range")
                    })?,
                )?
            };
            for local in aggregate_frame_locals.iter().rev() {
                let Type::Struct { size, .. } = local.declared_type else {
                    unreachable!("aggregate frame locals were filtered")
                };
                let slot_offset = interleaved_frame_layout
                    .as_ref()
                    .and_then(|layout| layout.offset(&local.name))
                    .unwrap_or(placements[&local.name]);
                self.frame_slots.insert(
                    local.name.clone(),
                    FrameSlot {
                        offset: slot_offset,
                        class: ValueClass::General,
                        size,
                        value_type: local.declared_type,
                        parameter_register: None,
                        is_array: false,
                    },
                );
            }
        }
        self.structured_aggregate_call_copy_plan = aggregate_call_copy_plan.clone();
        let guarded_structured_constant_return =
            saved_parameters.len() >= 2 && is_guarded_structured_constant_return(function);
        if !frame_arrays.is_empty()
            || !frame_scalar_parameters.is_empty()
            || !frame_scalar_locals.is_empty()
            || !aggregate_frame_locals.is_empty()
        {
            let mut extra_scalar_words = function
                .locals
                .iter()
                .filter(|local| {
                    local.array_length.is_none()
                        && !aggregate_frame_locals
                            .iter()
                            .any(|aggregate| aggregate.name == local.name)
                        && !frame_scalar_locals
                            .iter()
                            .any(|scalar| scalar.name == local.name)
                        && !deferred_saved_locals
                            .iter()
                            .any(|saved| saved.name == local.name)
                        && !eager_saved_locals
                            .iter()
                            .any(|saved| saved.name == local.name)
                        && !saved_float_locals
                            .iter()
                            .any(|saved| saved.name == local.name)
                        && !ephemeral_locals
                            .iter()
                            .any(|ephemeral| ephemeral.name == local.name)
                        && pure_local_alias(local).is_none()
                        && !is_call_result_local(&function.statements, &local.name)
                        && !is_transient_biased_scaled_member_call_local(
                            &function.statements,
                            &local.name,
                        )
                        && !is_transient_shifted_member_mask_call_local(
                            &function.statements,
                            &local.name,
                        )
                        && !is_transient_direct_call_argument_local(
                            &function.statements,
                            function.return_expression.as_ref(),
                            &local.name,
                        )
                        && body_uses_local(&function.statements, &local.name)
                })
                .count();
            if global_member_search_entry {
                // A linkage-first search-loop frame retains one source-local
                // table word for each deferred value, even when overlapping
                // lifetimes let several of those values share saved homes.
                // The table displaces the automatic arrays but not the value
                // allocator's physical saved-register count.
                extra_scalar_words += deferred_saved_locals.len();
            }
            let array_offset = if let Some(frame) = &variadic_output_frame {
                frame.array_offset
            } else {
                match self.behavior.frame_convention {
                    FrameConvention::Predecrement => 8,
                    FrameConvention::LinkageFirst => {
                        let words = if global_member_search_entry {
                            extra_scalar_words
                        } else {
                            self.entry_parameter_words
                                + extra_scalar_words
                                + 2 * usize::from(guarded_structured_constant_return)
                        };
                        8 + i16::try_from(words * 4).map_err(|_| {
                            Diagnostic::error("structured legacy local table is too large")
                        })?
                    }
                }
            };
            if !aggregate_frame_locals.is_empty() {
                let aggregate_base = u32::try_from(
                    array_offset
                        .checked_add(frame_array_bytes)
                        .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?,
                )
                .map_err(|_| Diagnostic::error("structured local frame is out of range"))?;
                let placements = plan_aggregate_frame_slots_from(
                    &aggregate_frame_locals,
                    &function.statements,
                    aggregate_base,
                )?;
                for local in &aggregate_frame_locals {
                    let Some(slot) = self.frame_slots.get_mut(&local.name) else {
                        return Err(Diagnostic::error(
                            "structured aggregate slot was not initialized",
                        ));
                    };
                    slot.offset = interleaved_frame_layout
                        .as_ref()
                        .and_then(|layout| layout.offset(&local.name))
                        .unwrap_or(placements[&local.name]);
                }
            }
            if self.behavior.frame_convention == FrameConvention::LinkageFirst {
                let occupied_base = if variadic_output_frame.is_some() {
                    8
                } else if frame_arrays.is_empty()
                    && linkage_first_scalar_local_table_bytes != 0
                {
                    8
                } else {
                    array_offset
                };
                let occupied = i32::from(occupied_base)
                    + i32::from(local_region_bytes)
                    + i32::try_from(4 * count).unwrap_or(i32::MAX);
                // The legacy value graph retains the terminal pointer alias as
                // one scalar slot but only rounds this frame to a doubleword.
                // Ordinary structured frames retain their 16-byte rounding.
                let alignment = if variadic_output_frame.is_some()
                    || folded_terminal_pointer_alias
                    || saved_float_count != 0
                    || (unused_frame_array && !aggregate_frame_locals.is_empty())
                    || !frame_scalar_parameters.is_empty()
                {
                    8
                } else {
                    16
                };
                let frame_size = if dense_frame
                    && !eager_saved_locals.is_empty()
                    && dense_retained_local_table_bytes != 0
                {
                    // Inline-body optimizer lanes and unobserved source
                    // scalars sit between the low automatic arrays and the
                    // addressable scalar table. This is an explicit local
                    // region, so unlike the fallback dense layout it does not
                    // need a second synthetic caller-linkage lane.
                    let arrays_end = align_offset(
                        array_offset
                            .checked_add(frame_array_bytes)
                            .ok_or_else(|| {
                                Diagnostic::error("structured local frame is too large")
                            })?,
                        4,
                    )
                    .ok_or_else(|| Diagnostic::error("structured local frame is too large"))?;
                    let scalar_bytes = i16::try_from(
                        4 * (frame_scalar_parameters.len() + frame_scalar_locals.len()),
                    )
                    .map_err(|_| Diagnostic::error("structured scalar frame is too large"))?;
                    let retained_end = arrays_end
                        .checked_add(dense_retained_local_table_bytes)
                        .and_then(|end| end.checked_add(scalar_bytes))
                        .ok_or_else(|| {
                            Diagnostic::error("structured retained local table is too large")
                        })?;
                    let occupied = i32::from(retained_end)
                        + i32::try_from(4 * count).unwrap_or(i32::MAX);
                    (occupied + 7) / 8 * 8
                } else if dense_frame && !eager_saved_locals.is_empty() {
                    // A dense legacy frame retains the caller-linkage word
                    // between the local region and its contiguous saved-GPR
                    // range. Dense linkage-first frames use the ABI's
                    // doubleword alignment; byte-sized automatic arrays can
                    // otherwise leave both the stack and `stmw` range
                    // misaligned.
                    (occupied + 8 + 7) / 8 * 8
                } else {
                    (occupied + alignment - 1) / alignment * alignment
                };
                plan.frame_size = i16::try_from(frame_size)
                    .map_err(|_| Diagnostic::error("structured legacy frame is too large"))?
                    .checked_add(path_reuse_frame_bytes)
                    .ok_or_else(|| Diagnostic::error("structured path-reuse frame is too large"))?;
            }
            if let Some(layout) = &interleaved_frame_layout {
                plan.frame_size = plan
                    .frame_size
                    .checked_add(layout.saved_area_gap_bytes())
                    .ok_or_else(|| {
                        Diagnostic::error("structured interleaved frame is too large")
                    })?;
            }
            let mut next_array_offset = array_offset;
            for array in structured_array_placement_order(frame_arrays) {
                next_array_offset = if let Some(offset) = interleaved_frame_layout
                    .as_ref()
                    .and_then(|layout| layout.offset(&array.name))
                {
                    offset
                } else {
                    align_offset(next_array_offset, array_stack_alignment(array))
                        .ok_or_else(|| {
                            Diagnostic::error("structured local frame is too large")
                        })?
                };
                let element_bytes = match array.declared_type {
                    Type::Struct { size, .. } => size,
                    value_type => u32::from(value_type.width() / 8),
                };
                let array_bytes = element_bytes
                    * u32::from(array.array_length.expect("frame array was gated"));
                self.frame_slots.insert(
                    array.name.clone(),
                    FrameSlot {
                        offset: next_array_offset,
                        class: ValueClass::General,
                        size: array_bytes,
                        value_type: array.declared_type,
                        parameter_register: None,
                        is_array: true,
                    },
                );
                if let Some(row_bytes) = array.row_bytes {
                    self.frame_row_bytes
                        .insert(array.name.clone(), row_bytes);
                }
                if interleaved_frame_layout.is_none() {
                    next_array_offset = next_array_offset
                        .checked_add(i16::try_from(array_bytes).map_err(|_| {
                            Diagnostic::error("structured automatic array is too large")
                        })?)
                        .ok_or_else(|| {
                            Diagnostic::error("structured local frame is too large")
                        })?;
                }
            }
            let mut scalar_offset =
                if let Some(frame) = &variadic_output_frame {
                    frame.scalar_offset
                } else if frame_arrays.is_empty() {
                    if self.behavior.frame_convention == FrameConvention::LinkageFirst
                        && !frame_scalar_parameters.is_empty()
                    {
                        array_offset
                            .checked_sub(
                                i16::try_from(frame_scalar_parameters.len() * 4).map_err(|_| {
                                    Diagnostic::error("structured scalar frame is too large")
                                })?,
                            )
                            .ok_or_else(|| {
                                Diagnostic::error("structured scalar frame is too large")
                            })?
                    } else {
                        frame_publication
                            .as_ref()
                            .map_or(
                                8 + linkage_first_scalar_local_table_bytes,
                                |_| CURSOR_OFFSET,
                            )
                    }
                } else if dense_retained_local_table_bytes != 0 {
                    let scalar_bytes = i16::try_from(
                        4 * (frame_scalar_parameters.len() + frame_scalar_locals.len()),
                    )
                    .map_err(|_| Diagnostic::error("structured scalar frame is too large"))?;
                    plan.frame_size
                        .checked_sub(i16::try_from(4 * frame_saved_count).map_err(|_| {
                            Diagnostic::error("structured saved frame is too large")
                        })?)
                        .and_then(|offset| offset.checked_sub(scalar_bytes))
                        .ok_or_else(|| {
                            Diagnostic::error("structured retained scalar frame is too large")
                        })?
                } else {
                    array_offset
                        .checked_sub(
                            i16::try_from(
                                (frame_scalar_parameters.len() + frame_scalar_locals.len()) * 4,
                            )
                            .map_err(|_| {
                                Diagnostic::error("structured scalar frame is too large")
                            })?,
                        )
                        .ok_or_else(|| Diagnostic::error("structured scalar frame is too large"))?
                };
            for parameter in &frame_scalar_parameters {
                let incoming = self
                    .locations
                    .get(&parameter.name)
                    .expect("address-taken parameter was assigned")
                    .register;
                self.frame_slots.insert(
                    parameter.name.clone(),
                    FrameSlot {
                        offset: scalar_offset,
                        class: ValueClass::General,
                        size: 4,
                        value_type: parameter.parameter_type,
                        parameter_register: Some(incoming),
                        is_array: false,
                    },
                );
                scalar_offset = scalar_offset
                    .checked_add(4)
                    .ok_or_else(|| Diagnostic::error("structured scalar frame is too large"))?;
            }
            for local in frame_scalar_locals.iter().rev() {
                self.frame_slots.insert(
                    local.name.clone(),
                    FrameSlot {
                        offset: scalar_offset,
                        class: class_of(local.declared_type)
                            .expect("address-taken scalar class was checked"),
                        size: 4,
                        value_type: local.declared_type,
                        parameter_register: None,
                        is_array: false,
                    },
                );
                scalar_offset = scalar_offset
                    .checked_add(4)
                    .ok_or_else(|| Diagnostic::error("structured scalar frame is too large"))?;
            }
            if let Some(publication) = &frame_publication {
                self.frame_slots.insert(
                    publication.parameter.clone(),
                    FrameSlot {
                        offset: OWNER_OFFSET,
                        class: ValueClass::General,
                        size: 4,
                        value_type: Type::Pointer(Pointee::Pointer),
                        parameter_register: None,
                        is_array: false,
                    },
                );
            }
            for array in frame_arrays {
                let (pointee, stride) = match array.declared_type {
                    Type::Struct { size, .. } => (None, Some(u32::from(size))),
                    value_type => (pointee_of_type(value_type), None),
                };
                if pointee.is_none() && stride.is_none() {
                    return Err(Diagnostic::error(
                        "structured frame array has no element representation",
                    ));
                }
                self.locations.insert(
                    array.name.clone(),
                    Location {
                        class: ValueClass::General,
                        register: GENERAL_SCRATCH,
                        signed: false,
                        width: 32,
                        pointee,
                        stride,
                    },
                );
            }
        }
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = if array_pool_plan.is_some() {
            (frame_first_saved as u8..=31).rev().collect()
        } else {
            logical_saved_homes
        };
        self.legacy_callee_saved_frame_layout = if global_member_address_cache_plan
            .as_ref()
            .is_some_and(|plan| plan.defer_until_first_use)
        {
            LegacyCalleeSavedFrameLayout::RetainDeferredGlobalMemberAddressLane
        } else if global_member_address_cache_plan.is_some()
            || unused_frame_array
            || !frame_scalar_parameters.is_empty()
            || !frame_scalar_locals.is_empty()
        {
            // An unused source-level scratch array still occupies its declared
            // bytes, but creates no retained value-table lane. Addressable
            // parameters likewise already occupy the linkage-first incoming
            // value table. Address-taken locals own an explicit stack region
            // too. Their logical frames account for the local region and every
            // saved home, so none of these cases reserves another lane.
            LegacyCalleeSavedFrameLayout::PreserveLogicalSize
        } else if !with_frame_array
            && eager_saved_locals.len() == 1
            && saved_parameters.is_empty()
            && deferred_home_plan.group_count == 1
            && self.legacy_inline_expansion_frame_bytes != 0
        {
            LegacyCalleeSavedFrameLayout::RetainEagerLocalLane
        } else if suppressed_constant_lane {
            // A store constant which lost its fifth saved home still retains
            // its optimizer value lane in build 163's logical frame.
            LegacyCalleeSavedFrameLayout::RetainDeferredLocalLane
        } else if retains_unobserved_local_lane {
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTableAndDeferredLocalLane
        } else if is_plain_short_circuit_call_if(function)
            && self.entry_parameter_words <= 2
        {
            // A single call-bearing conjunction whose incoming values fit one
            // word pair has no distinct retained local table. Three or more
            // words span multiple pairs and retain the full entry table.
            LegacyCalleeSavedFrameLayout::InferFromValueOrigin
        } else if guarded_structured_constant_return {
            LegacyCalleeSavedFrameLayout::RetainGuardedEntryParameterTable
        } else if deferred_saved_locals.len() >= 2
            && eager_saved_locals.is_empty()
            && saved_parameters.is_empty()
            && deferred_saved_locals
                .iter()
                .all(|local| local.name.starts_with("__mwcc_retained_constant_"))
        {
            // Compiler-created store constants are value versions, not source
            // locals with retained stack-table identities. Multiple versions
            // occupy only their saved homes.
            LegacyCalleeSavedFrameLayout::InferFromValueOrigin
        } else if function
            .locals
            .iter()
            .any(|local| local.name.starts_with("__mwcc_retained_constant_"))
            || (deferred_home_plan.group_count == 1 && retained_deferred_local_lane)
        {
            LegacyCalleeSavedFrameLayout::RetainDeferredLocalLane
        } else {
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTable
        };
        let pooled_dense_inline_save = array_pool_plan.is_some();
        let dense_save_helper = dense_saved_range
            && self.behavior.frame_convention == FrameConvention::Predecrement
            && !pooled_dense_inline_save;
        let dense_inline_save =
            dense_saved_range && self.behavior.frame_convention == FrameConvention::LinkageFirst;
        if dense_saved_range && array_pool_plan.is_none() {
            self.output.pre_scheduled = true;
        }
        if pooled_dense_inline_save {
            self.output.instructions.extend([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -plan.frame_size,
                },
                Instruction::MoveFromLinkRegister { d: 0 },
            ]);
            self.emit_structured_array_pool_base_high();
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: plan.frame_size + 4,
            });
            self.emit_structured_array_pool_base_low(
                array_pool_plan
                    .as_ref()
                    .expect("pooled frame has an array-pool plan"),
            );
            self.output
                .instructions
                .push(Instruction::StoreMultipleWord {
                    s: frame_first_saved as u8,
                    a: 1,
                    offset: plan.frame_size - 4 * frame_saved_count as i16,
                });
        } else if dense_inline_save {
            self.output.instructions.extend([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                },
            ]);
            if dense_entry_prefix {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
            }
            self.output.instructions.extend([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -plan.frame_size,
                },
                Instruction::StoreMultipleWord {
                    s: frame_first_saved as u8,
                    a: 1,
                    offset: plan.frame_size - 4 * frame_saved_count as i16,
                },
            ]);
        } else if aggregate_call_copy_plan.is_some()
            && self.behavior.frame_convention == FrameConvention::LinkageFirst
            && homes.is_empty()
        {
            self.output.instructions.extend([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -plan.frame_size,
                },
            ]);
        } else {
            self.output.instructions.extend([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -plan.frame_size,
                },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: plan.frame_size + 4,
                },
            ]);
        }
        if let Some(cache) = global_base_cache_plan {
            let register = self.fresh_virtual_general_preferring(4);
            self.emit_global_array_base(&cache.global, cache.total_size, register)?;
            self.structured_global_base_cache =
                Some(crate::generator::StructuredGlobalBaseCache {
                    global: cache.global,
                    register,
                });
        }
        if let (Some(cache), Some(register)) = (
            global_member_address_cache_plan,
            standalone_global_member_address_home,
        ) {
            let initialized = !cache.defer_until_first_use;
            if initialized {
                let base = self.fresh_virtual_general_preferring(3);
                self.emit_global_array_base(&cache.global, cache.total_size, base)?;
                self.output.instructions.push(Instruction::AddImmediate {
                    d: register,
                    a: base,
                    immediate: cache.offset,
                });
            }
            self.structured_global_member_address_cache =
                Some(crate::generator::StructuredGlobalMemberAddressCache {
                    global: cache.global,
                    total_size: cache.total_size,
                    offset: cache.offset,
                    register,
                    initialized,
                });
            self.emit_structured_saved_home_store(
                register,
                usize::from(standalone_data_anchor_home.is_some()),
                plan.frame_size,
            );
        }
        if let Some(register) = data_section_anchor_home {
            if !dense_saved_range {
                let save_slot = reused_data_anchor_slot.unwrap_or(0);
                self.emit_structured_saved_home_store(register, save_slot, plan.frame_size);
            }
            let high = self.fresh_virtual_general_preferring(5);
            let anchor_symbol = self
                .data_section_anchor
                .as_ref()
                .map(|anchor| anchor.anchor_symbol.clone())
                .expect("a data-section anchor home requires an anchor plan");
            self.record_relocation(RelocationKind::Addr16Ha, &anchor_symbol);
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: high,
                    a: 0,
                    immediate: 0,
                });
            self.record_relocation(RelocationKind::Addr16Lo, &anchor_symbol);
            self.output.instructions.push(Instruction::AddImmediate {
                d: register,
                a: high,
                immediate: 0,
            });
        }
        if dense_save_helper {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 11,
                a: 1,
                immediate: plan.frame_size,
            });
            let helper = format!("_savegpr_{frame_first_saved}");
            self.record_relocation(RelocationKind::Rel24, &helper);
            self.output
                .instructions
                .push(Instruction::BranchAndLink { target: helper });
        }
        if aggregate_call_copy_plan.is_some() {
            let image_locals: Vec<_> = aggregate_frame_locals
                .iter()
                .filter(|local| {
                    aggregate_call_copy_plan.as_ref().is_some_and(|copy_plan| {
                        copy_plan
                            .copies
                            .iter()
                            .any(|copy| copy.local == local.name)
                    })
                })
                .collect();
            let [first, second] = image_locals.as_slice() else {
                return Err(Diagnostic::error(
                    "structured aggregate copy initialization lost its source objects",
                ));
            };
            for (local, destination) in [(*first, 3), (*second, 0)] {
                let image = local
                    .data_bytes
                    .as_ref()
                    .expect("aggregate copy planning required a source image");
                let bits = u32::from_be_bytes([image[0], image[1], image[2], image[3]]);
                self.load_word_constant(destination, bits);
            }
            for (local, source) in [(*first, 3), (*second, 0)] {
                let offset = self
                    .frame_slots
                    .get(&local.name)
                    .expect("aggregate copy source has a frame slot")
                    .offset;
                self.output.instructions.push(Instruction::StoreWord {
                    s: source,
                    a: 1,
                    offset,
                });
            }
        }

        let paired_eager_deferred_homes = self.legacy_callee_saved_frame_layout
            == LegacyCalleeSavedFrameLayout::RetainEagerLocalLane
            && count == 2;
        let batched_saved_home_stores = unused_array_two_homes
            || compact_aggregate_scratch_pair
            || paired_eager_deferred_homes
            || unused_array_eager_homes
            || sequenced_callback_wait_layout
            || loop_member_receiver_layout.is_some()
            || saved_home_stores_precede_initialization(
                self.behavior.frame_convention,
                eager_saved_locals.len(),
                saved_parameters.len(),
                deferred_home_plan.group_count,
            );

        let saved_parameter_base = eager_saved_locals.len();
        let dense_entry_parameter_copies =
            dense_entry_owns_parameter_copies(dense_frame, saved_parameter_base);
        let mut saved_parameter_homes = Vec::with_capacity(saved_parameters.len());
        for (parameter_index, parameter) in saved_parameters.iter().enumerate() {
            let home = homes[saved_parameter_base + parameter_index];
            let incoming = self
                .locations
                .get(&parameter.name)
                .expect("eligibility checked")
                .register;
            saved_parameter_homes.push((parameter.name.clone(), home, incoming));
        }
        let deferred_home_base = saved_parameter_base + saved_parameter_homes.len();
        let publication_entry_emitted = if let Some(publication) = &frame_publication {
            let incoming = self
                .locations
                .get(&publication.parameter)
                .expect("publication parameter was eligibility checked")
                .register;
            let cursor_slot = self
                .frame_slots
                .get(&publication.cursor)
                .copied()
                .expect("publication cursor has a frame slot");
            let [last_extent, first_extent, base] = saved_parameter_homes.as_slice() else {
                return Err(Diagnostic::error(
                    "dense cursor publication requires three retained parameters",
                ));
            };
            self.output.instructions.extend([
                Instruction::StoreWord {
                    s: incoming,
                    a: 1,
                    offset: OWNER_OFFSET,
                },
                Instruction::move_register(base.1, base.2),
                Instruction::LoadWord {
                    d: incoming,
                    a: incoming,
                    offset: 0,
                },
                Instruction::move_register(first_extent.1, first_extent.2),
                Instruction::move_register(last_extent.1, last_extent.2),
                Instruction::StoreWord {
                    s: incoming,
                    a: 1,
                    offset: cursor_slot.offset,
                },
            ]);
            true
        } else {
            false
        };
        let stagger_dense_parameter_copies =
            dense_saved_range && saved_parameter_base != 0 && saved_parameter_homes.len() >= 2;
        let ordered_entry_save_order = loop_member_receiver_layout
            .as_ref()
            .map(|layout| layout.save_order().to_vec())
            .or_else(|| {
                sequenced_callback_wait_layout
                    .then_some(sequenced_callback_wait_save_order().to_vec())
            });
        let ordered_entry_emitted =
            if let Some(save_order) = ordered_entry_save_order {
                for home_index in save_order {
                    let home = homes[home_index];
                    self.emit_structured_saved_home_store(
                        home,
                        frame_slot_for_home(home_index),
                        plan.frame_size,
                    );
                    if (saved_parameter_base..deferred_home_base)
                        .contains(&home_index)
                    {
                        let (_, parameter_home, incoming) =
                            &saved_parameter_homes[home_index - saved_parameter_base];
                        debug_assert_eq!(*parameter_home, home);
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: home,
                            a: *incoming,
                            immediate: 0,
                        });
                    }
                }
                true
            } else {
                false
            };
        if batched_saved_home_stores && !ordered_entry_emitted {
            if !dense_saved_range {
                for (home_index, &home) in homes[..saved_parameter_base].iter().enumerate() {
                    self.emit_structured_saved_home_store(
                        home,
                        frame_slot_for_home(home_index),
                        plan.frame_size,
                    );
                }
            }
            if stagger_dense_parameter_copies {
                let (name, home, incoming) = saved_parameter_homes
                    .last()
                    .expect("staggered copies require saved parameters");
                self.output
                    .instructions
                    .push(Instruction::move_register(*home, *incoming));
                self.locations
                    .get_mut(name)
                    .expect("eligibility checked")
                    .register = *home;
            } else if pooled_dense_inline_save {
                self.emit_structured_array_pool_parameter_copies(&saved_parameter_homes);
            } else {
                for (parameter_index, (_, home, incoming)) in
                    saved_parameter_homes.iter().enumerate()
                {
                    if !dense_saved_range {
                        self.emit_structured_saved_home_store(
                            *home,
                            frame_slot_for_home(saved_parameter_base + parameter_index),
                            plan.frame_size,
                        );
                    }
                    self.output
                        .instructions
                        .push(Instruction::move_register(*home, *incoming));
                }
            }
            if !dense_saved_range {
                for (home_index, &home) in homes.iter().enumerate().skip(deferred_home_base) {
                    let slot = frame_slot_for_home(home_index);
                    if reused_data_anchor_slot != Some(slot) {
                        self.emit_structured_saved_home_store(home, slot, plan.frame_size);
                    }
                }
            }
        }

        if let Some(plan) = initializer_live_in {
            self.emit_initializer_live_in(plan);
        }
        // Saved locals are partitioned by register class for home allocation,
        // but their initializers still obey source declaration order. Prime an
        // earlier float local before an eager GPR local whose initializer reads
        // it (`float f = ...; int i = f;`). Without this bridge the GPR pass
        // mistakes `f` for a global because the FPR pass has not installed its
        // location yet.
        let mut early_saved_float_homes = std::collections::HashMap::new();
        let mut early_ephemeral_float_homes = std::collections::HashMap::new();
        let mut home_index = 0;
        let mut deferred_round_up_base = None;
        let mut dense_eager_consumed_statements = 0usize;
        for local in eager_saved_locals {
            let home = homes[home_index];
            home_index += 1;
            if !batched_saved_home_stores && !dense_saved_range {
                self.emit_structured_saved_home_store(
                    home,
                    frame_slot_for_home(home_index - 1),
                    plan.frame_size,
                );
            }
            let initializer = local.initializer.as_ref().expect("partitioned as eager");
            if complement_product_pair.is_none() {
                for dependency in &saved_float_locals {
                    if self.locations.contains_key(&dependency.name)
                        || !crate::analysis::expression_reads_name(
                            initializer,
                            &dependency.name,
                        )
                    {
                        continue;
                    }
                    let dependency_initializer = dependency
                        .initializer
                        .as_ref()
                        .ok_or_else(|| {
                            Diagnostic::error("a saved float dependency has no initializer")
                        })?;
                    let group = saved_float_plan.group(&dependency.name);
                    let preferred = if saved_float_parameters.is_empty() {
                        saved_float_home_preference(
                            group,
                            saved_float_plan.group_count,
                            compact_aggregate_scratch_pair,
                        )
                    } else {
                        31u8.saturating_sub(
                            u8::try_from(saved_float_parameters.len() + group)
                                .unwrap_or(17)
                                .min(17),
                        )
                    };
                    let home = self.fresh_virtual_float_preferring(preferred);
                    self.evaluate(
                        dependency_initializer,
                        dependency.declared_type,
                        home,
                    )?;
                    self.locations.insert(
                        dependency.name.clone(),
                        Location {
                            class: ValueClass::Float,
                            register: home,
                            signed: true,
                            width: dependency.declared_type.width(),
                            pointee: None,
                            stride: None,
                        },
                    );
                    early_saved_float_homes.insert(dependency.name.clone(), home);
                }
                for dependency in &ephemeral_locals {
                    if class_of(dependency.declared_type).ok() != Some(ValueClass::Float)
                        || self.locations.contains_key(&dependency.name)
                        || !crate::analysis::expression_reads_name(
                            initializer,
                            &dependency.name,
                        )
                    {
                        continue;
                    }
                    let dependency_initializer = dependency
                        .initializer
                        .as_ref()
                        .ok_or_else(|| {
                            Diagnostic::error(
                                "an ephemeral float dependency has no initializer",
                            )
                        })?;
                    let home = self.fresh_virtual_float();
                    self.evaluate(
                        dependency_initializer,
                        dependency.declared_type,
                        home,
                    )?;
                    self.locations.insert(
                        dependency.name.clone(),
                        Location {
                            class: ValueClass::Float,
                            register: home,
                            signed: true,
                            width: dependency.declared_type.width(),
                            pointee: None,
                            stride: None,
                        },
                    );
                    early_ephemeral_float_homes
                        .insert(dependency.name.clone(), home);
                }
            }
            let initializer_start = self.output.instructions.len();
            let mut location_register = home;
            let is_round_up_base = dense_eager_round_up
                .as_ref()
                .is_some_and(|round_up| round_up.base_name == local.name);
            let is_rounded_pointer = dense_eager_round_up
                .as_ref()
                .is_some_and(|round_up| round_up.pointer_name == local.name);
            if is_round_up_base {
                let temporary = self.fresh_virtual_general_preferring(3);
                self.evaluate(initializer, local.declared_type, temporary)?;
                location_register = temporary;
                deferred_round_up_base = Some((local.name.clone(), home, temporary));
            } else if is_rounded_pointer {
                let round_up = dense_eager_round_up
                    .as_ref()
                    .expect("rounded pointer was classified");
                let (base_name, base_home, temporary) = deferred_round_up_base
                    .as_ref()
                    .expect("rounded pointer base must be initialized first");
                debug_assert_eq!(base_name, &round_up.base_name);
                let substitutions = std::collections::HashMap::from([(
                    round_up.pointer_name.clone(),
                    Expression::Variable(round_up.base_name.clone()),
                )]);
                let rounded =
                    crate::value_tracking::substitute(&round_up.rounded_expression, &substitutions);
                self.evaluate(&rounded, local.declared_type, home)?;
                self.output
                    .instructions
                    .push(Instruction::move_register(*base_home, *temporary));
                self.locations
                    .get_mut(base_name)
                    .expect("rounded pointer base was initialized")
                    .register = *base_home;
                dense_eager_consumed_statements = round_up.statement_index + 1;
            } else if complement_product_pair
                .as_ref()
                .is_none_or(|pair| !pair.interleaves_general_initializer(&local.name))
            {
                let handled_loop_cursor =
                    if let Some(layout) = &loop_member_receiver_layout {
                        layout.try_emit_cursor_initializer(
                            self,
                            &local.name,
                            initializer,
                            home,
                        )?
                    } else {
                        false
                    };
                let handled_dense_global = stagger_dense_parameter_copies
                    && home_index == 1
                    && self.try_emit_dense_eager_global_array_initializer(initializer, home)?;
                if !handled_loop_cursor
                    && !handled_dense_global
                    && !self.try_emit_structured_wide_saved_initializer(initializer, home)
                {
                    self.evaluate(initializer, local.declared_type, home)?;
                }
            }
            if stagger_dense_parameter_copies && home_index == 1 {
                self.schedule_dense_eager_initializer(initializer_start);
                for (_, home, incoming) in saved_parameter_homes
                    .iter()
                    .take(saved_parameter_homes.len() - 1)
                {
                    self.output
                        .instructions
                        .push(Instruction::move_register(*home, *incoming));
                }
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: location_register,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        for (_, home, incoming) in &saved_parameter_homes {
            home_index += 1;
            if !batched_saved_home_stores {
                if !dense_saved_range {
                    self.emit_structured_saved_home_store(
                        *home,
                        frame_slot_for_home(home_index - 1),
                        plan.frame_size,
                    );
                }
                if !dense_entry_parameter_copies {
                    self.output
                        .instructions
                        .push(Instruction::move_register(*home, *incoming));
                }
            }
        }
        debug_assert_eq!(home_index, deferred_home_base);
        for group in 0..deferred_home_plan.group_count {
            let slot_index = parameter_home_reuse.home_index(group);
            if slot_index < deferred_home_base {
                continue;
            }
            let home = homes[slot_index];
            let frame_slot = frame_slot_for_home(slot_index);
            if !batched_saved_home_stores
                && !dense_saved_range
                && reused_data_anchor_slot != Some(frame_slot)
            {
                self.emit_structured_saved_home_store(
                    home,
                    frame_slot,
                    plan.frame_size,
                );
            }
        }
        for local in deferred_saved_locals {
            let group = deferred_home_plan.group(&local.name);
            let home = homes[parameter_home_reuse.home_index(group)];
            if self.inline_source_call_survivors.contains(&local.name) {
                if let mwcc_vreg::Reg::Virtual(register) =
                    mwcc_vreg::Reg::from_field(home, mwcc_vreg::Class::General)
                {
                    self.forced_general_callee_saved.insert(register);
                }
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        self.callee_saved_float = self
            .callee_saved_float
            .max(u8::try_from(saved_float_count).unwrap_or(18));
        for (parameter_index, parameter) in saved_float_parameters.iter().enumerate() {
            let incoming = self
                .locations
                .get(&parameter.name)
                .expect("eligibility checked")
                .register;
            let preferred =
                31u8.saturating_sub(u8::try_from(parameter_index).unwrap_or(17).min(17));
            let home = self.fresh_virtual_float_preferring(preferred);
            self.output
                .instructions
                .push(Instruction::FloatMove { d: home, b: incoming });
            self.locations.insert(
                parameter.name.clone(),
                Location {
                    class: ValueClass::Float,
                    register: home,
                    signed: true,
                    width: parameter.parameter_type.width(),
                    pointee: None,
                    stride: None,
                },
            );
        }
        let saved_float_homes: Vec<_> = saved_float_locals
            .iter()
            .map(|local| {
                if let Some(&home) = early_saved_float_homes.get(&local.name) {
                    return home;
                }
                let group = saved_float_plan.group(&local.name);
                let preferred = if saved_float_parameters.is_empty() {
                    saved_float_home_preference(
                        group,
                        saved_float_plan.group_count,
                        compact_aggregate_scratch_pair,
                    )
                } else {
                    31u8.saturating_sub(
                        u8::try_from(saved_float_parameters.len() + group)
                            .unwrap_or(17)
                            .min(17),
                    )
                };
                self.fresh_virtual_float_preferring(preferred)
            })
            .collect();
        if let Some(pair) = &complement_product_pair {
            let destinations = pair.product_names().map(|name| {
                saved_float_locals
                    .iter()
                    .zip(&saved_float_homes)
                    .find_map(|(local, home)| (local.name == name).then_some(*home))
                    .expect("the paired-product plan names two saved float locals")
            });
            self.emit_structured_complement_product_pair(pair, destinations)?;
        } else {
            for (local, &home) in saved_float_locals.iter().zip(&saved_float_homes) {
                if !early_saved_float_homes.contains_key(&local.name) {
                    if let Some(initializer) = &local.initializer {
                        self.evaluate(initializer, local.declared_type, home)?;
                    }
                }
            }
        }
        for (local, home) in saved_float_locals
            .into_iter()
            .zip(saved_float_homes)
        {
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::Float,
                    register: home,
                    signed: true,
                    width: local.declared_type.width(),
                    pointee: None,
                    stride: None,
                },
            );
        }
        self.try_preload_ephemeral_float_compare_literal(function, &ephemeral_locals)?;
        // Initializers are evaluated at declaration time, while an incoming
        // parameter still has its entry-register alias. MWCC can preserve that
        // alias after copying the value to a saved home (`mr r31,r3; lwz ...,r3`)
        // and switches subsequent body uses to the home only after declarations.
        for local in &ephemeral_locals {
            if early_ephemeral_float_homes.contains_key(&local.name) {
                continue;
            }
            let class = class_of(local.declared_type).expect("eligibility checked");
            let transient_float_call_result = class == ValueClass::Float
                && transient_condition_call_result_callee(
                    &function.statements,
                    &local.name,
                )
                .is_some_and(|callee| {
                    matches!(
                        self.call_return_types.get(callee),
                        Some(Type::Float | Type::Double)
                    )
                });
            if transient_float_call_result {
                self.transient_condition_float_call_results
                    .insert(local.name.clone());
            }
            let alias = pure_local_alias(local)
                .and_then(|name| self.locations.get(name))
                .filter(|location| location.class == class)
                .map(|location| location.register);
            let temporary = alias.unwrap_or_else(|| match class {
                ValueClass::General if rounded_byte_pointer == Some(local.name.as_str()) => {
                    self.fresh_virtual_general_preferring(Eabi::general_result().number)
                }
                ValueClass::General if is_frame_address_null_select(function, &local.name) => {
                    self.fresh_virtual_general_preferring(4)
                }
                ValueClass::General
                    if aggregate_call_copy_plan.is_some()
                        && transient_call_argument_register(
                            &function.statements,
                            &local.name,
                        )
                        .is_some() =>
                {
                    self.fresh_virtual_general_preferring(
                        transient_call_argument_register(
                            &function.statements,
                            &local.name,
                        )
                        .expect("aggregate-call companion preference was checked"),
                    )
                }
                ValueClass::General
                    if matches!(
                        local.initializer,
                        Some(
                            Expression::Call { .. }
                                | Expression::CallThrough { .. }
                                | Expression::VirtualCall { .. }
                        )
                    ) =>
                {
                    // The initializer defines this local only after the call
                    // returns. Ephemeral planning also proves that its value
                    // crosses no later call, so it can remain in the fixed EABI
                    // result home without a copy out and back.
                    mwcc_target::Eabi::general_result().number
                }
                ValueClass::General
                    if is_sequenced_call_result_local(&function.statements, &local.name) =>
                {
                    // Inline value composition sequences the callee's effects
                    // before its terminal call with comma operators. Liveness
                    // has already proved this local crosses no later call, so
                    // the surviving call value can stay in the EABI result
                    // register just like a bare `result = call()`.
                    mwcc_target::Eabi::general_result().number
                }
                ValueClass::General => {
                    if self.canonical_boolean_locals.contains(&local.name) {
                        self.fresh_virtual_general_preferring(GENERAL_SCRATCH)
                    } else if let Some(register) =
                        dense_loop_carried.preference_for(&local.name)
                    {
                        self.fresh_virtual_general_preferring(register)
                    } else {
                        self.fresh_virtual_general()
                    }
                }
                ValueClass::Float => self.fresh_virtual_float_preferring(
                    self.ephemeral_float_home_preference(function, &ephemeral_locals),
                ),
            });
            if alias.is_none() {
                if let Some(initializer) = &local.initializer {
                    self.evaluate(initializer, local.declared_type, temporary)?;
                }
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
                    register: temporary,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        for local in &frame_scalar_locals {
            if frame_publication
                .as_ref()
                .is_some_and(|publication| publication.cursor == local.name)
            {
                continue;
            }
            if let Some(initializer) = &local.initializer {
                self.emit_store(&Expression::Variable(local.name.clone()), initializer)?;
            }
        }
        for parameter in &frame_scalar_parameters {
            let slot = self.frame_slots[&parameter.name];
            self.output.instructions.push(crate::frame::spill_instruction(
                slot.parameter_register
                    .expect("address-taken parameter has an incoming register"),
                slot,
            ));
        }
        self.emit_structured_frame_array_initializers(
            frame_arrays,
            frame_array_image_sources,
        )?;
        if let Some(cache) = global_index_cache_plan {
            let source = saved_parameter_homes
                .iter()
                .find_map(|(name, home, _)| (name == &cache.index).then_some(*home))
                .or_else(|| self.lookup_general(&cache.index))
                .ok_or_else(|| {
                    Diagnostic::error("structured global-index cache has no source register")
                })?;
            // The compact pooled-copy forms finish using their saved base at
            // the same boundary where the scaled global index is born. MWCC
            // reuses that physical register (CUT1 r29, CUT2 r21); the full
            // 24-word transaction retains its established r14 preference.
            let scaled = self.fresh_virtual_general_preferring(
                array_pool_plan
                    .as_ref()
                    .map_or(14, |plan| plan.first_saved_register),
            );
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                source,
                cache.stride,
            )?;
            let retained_element = cache
                .retain_element
                .then(|| self.fresh_virtual_general_preferring(15));
            self.structured_global_index_cache =
                Some(crate::generator::StructuredGlobalIndexCache {
                    global: cache.global,
                    index: cache.index,
                    stride: cache.stride,
                    scaled,
                    retained_element,
                    retained_element_initialized: false,
                });
        }
        self.plan_structured_float_handoff(function, &ephemeral_locals);
        let dense_statement_start = if dense_frame {
            if global_member_search_entry || saved_parameter_base != 0 {
                dense_eager_consumed_statements
            } else {
                match self.emit_structured_dense_frame_entry(function, &saved_parameter_homes)? {
                    Some(statement_start) => statement_start,
                    None => {
                        // Dense save-helper frames do not inherently require a
                        // global-array address definition at entry. When the
                        // body starts with a guard, packet store, or ordinary
                        // scalar assignment, home the preserved parameters in
                        // source order and lower from statement zero.
                        if !batched_saved_home_stores && !publication_entry_emitted {
                            for (_, home, incoming) in &saved_parameter_homes {
                                self.output
                                    .instructions
                                    .push(Instruction::move_register(*home, *incoming));
                            }
                        }
                        0
                    }
                }
            }
        } else {
            0
        };
        // Analysis uses the fully canonicalized switch tree above. Emission
        // instead retains proven-dense source switches so their dedicated
        // owner can share fallthrough bodies and materialize a jump table.
        let structured_function = emission_function;
        let alias_statements = if dense_frame {
            &structured_function.statements[dense_statement_start..]
        } else {
            structured_function.statements.as_slice()
        };
        // A declaration initializer call has already clobbered the incoming
        // ABI registers before the first statement. Only extend an entry alias
        // into that statement when all declaration initializers are call-free.
        let initializers_preserve_entry_alias = function.locals.iter().all(|local| {
            local.data_bytes.is_none()
                && local
                    .initializer
                    .as_ref()
                    .is_none_or(|initializer| !crate::analysis::expression_has_call(initializer))
        });
        let entry_parameter_alias = (!dense_inline_save && initializers_preserve_entry_alias)
            .then(|| {
                plan_first_call_alias(
                    alias_statements,
                    &saved_parameter_homes,
                    &function.parameters,
                )
            })
            .flatten();
        for (name, home, _) in &saved_parameter_homes {
            if entry_parameter_alias
                .as_ref()
                .is_some_and(|alias| alias.name == *name)
            {
                continue;
            }
            self.locations
                .get_mut(name)
                .expect("eligibility checked")
                .register = *home;
        }

        let mut return_branches = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut pending_gotos = Vec::new();
        let preassigned_local_names: std::collections::HashSet<String> =
            self.locations.keys().cloned().collect();
        let statement_start = if dense_frame {
            dense_statement_start
        } else if entry_parameter_alias
            .as_ref()
            .is_some_and(|alias| alias.boundary == EntryAliasBoundary::AfterFirstStatement)
        {
            let alias = entry_parameter_alias.as_ref().expect("checked above");
            self.emit_structured_statements(
                &structured_function.statements[..1],
                structured_function,
                &ephemeral_locals,
                false,
                &mut return_branches,
                &mut label_positions,
                &mut pending_gotos,
                &mut None,
            )?;
            self.locations
                .get_mut(&alias.name)
                .expect("planned saved parameter")
                .register = alias.home;
            self.release_dead_ephemeral_float_locations(
                &ephemeral_locals,
                &structured_function.statements[1..],
            );
            1
        } else {
            0
        };
        let statement_start = statement_start.max(
            complement_product_pair
                .as_ref()
                .map_or(0, StructuredComplementProductPair::consumed_statement_prefix),
        );
        let mut condition_alias = entry_parameter_alias
            .filter(|alias| alias.boundary == EntryAliasBoundary::AfterFirstConditionTerm);
        self.emit_structured_statements(
            &structured_function.statements[statement_start..],
            structured_function,
            &ephemeral_locals,
            true,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut condition_alias,
        )?;
        // Resolve symbolic gotos while their recorded instruction and label
        // indices still refer to the freshly emitted stream. The scheduling
        // passes below own branch-target remapping when they move or remove
        // instructions; postponing resolution until after those passes leaves
        // the original placeholder behind when its branch itself moved.
        for (branch, label) in pending_gotos {
            let target = label_positions.get(&label).copied().ok_or_else(|| {
                Diagnostic::error(format!(
                    "structured forward branch targets an unknown label '{label}'"
                ))
            })?;
            match &mut self.output.instructions[branch] {
                Instruction::Branch {
                    target: branch_target,
                }
                | Instruction::BranchConditionalForward {
                    target: branch_target,
                    ..
                } => *branch_target = target,
                _ => {}
            }
        }
        self.fold_structured_conditional_gotos();
        resolve_structured_switch_joins(&mut self.output.instructions);
        thread_forward_unconditional_branch_chains(&mut self.output.instructions);
        if let Some(layout) = &loop_member_receiver_layout {
            layout.coalesce_receiver_load(self, homes[0], homes[3]);
        }
        if let Some(layout) = &object_collision_loop_layout {
            let scheduled =
                schedule_object_collision_loop_entry(self, layout.entry_homes(&homes));
            self.structured_object_collision_loop_entry = scheduled;
            if capture {
                eprintln!("structured object collision entry scheduled: {scheduled}");
            }
        }
        let forwardable_frame_scalar_offsets = frame_scalar_locals
            .iter()
            .filter(|local| !local.is_volatile)
            .filter_map(|local| self.frame_slots.get(&local.name).map(|slot| slot.offset))
            .collect();
        self.forward_adjacent_frame_scalar_values(&forwardable_frame_scalar_offsets);
        if !allocator_cursor_preferences.is_empty() {
            self.schedule_allocator_cursor_entry();
        }
        self.schedule_structured_entry_zero_store(function);
        self.schedule_shared_switch_entry_transactions(structured_function);
        self.schedule_structured_shared_member_arguments(function);
        self.schedule_entry_member_call_argument_reuse();
        self.schedule_repeated_member_address_call_guards();
        self.schedule_guarded_member_receiver_reuse();
        self.schedule_guarded_member_classifier_chain();
        self.schedule_guarded_float_argument();
        self.schedule_structured_float_store_call_arguments();
        self.schedule_transient_condition_float_call_entry(function);
        self.schedule_entry_initialized_saved_float(function);
        self.schedule_structured_aggregate_constructor();
        self.schedule_structured_member_scales_and_compare();
        self.schedule_structured_frame_digit_pair();
        self.schedule_structured_virtual_calls();
        self.schedule_leading_member_store_call();
        if exclusive_arm_home_layout.is_some() {
            self.schedule_exclusive_arm_entry();
            self.schedule_exclusive_arm_wide_snapshot();
            self.schedule_exclusive_arm_callback_setup();
            self.schedule_exclusive_arm_float_entry();
            self.schedule_exclusive_arm_mask_packet();
            self.schedule_exclusive_arm_object_creation();
        }
        if dense_entry_prefix {
            self.schedule_structured_prefixed_frame_entry();
        }
        if !call_accumulators.is_empty() {
            self.schedule_structured_call_accumulator_chain();
        }
        if dense_frame {
            self.schedule_structured_frame_store_call();
        }
        if dense_inline_save {
            let logical_call_result_homes: Vec<u8> = function
                .locals
                .iter()
                .filter(|local| {
                    has_split_value_version(function, &local.name)
                        && !preassigned_local_names.contains(&local.name)
                })
                .filter_map(|local| self.lookup_general(&local.name))
                .collect();
            let recycled_call_result_homes: Vec<u8> = function
                .locals
                .iter()
                .filter(|local| {
                    has_split_value_version(function, &local.name)
                        && preassigned_local_names.contains(&local.name)
                })
                .filter_map(|local| self.lookup_general(&local.name))
                .collect();
            self.normalize_structured_frame_argument_copies(
                first_saved as u8,
                &logical_call_result_homes,
                &recycled_call_result_homes,
            );
        }
        self.schedule_structured_signed_conversion_pair();
        self.reuse_structured_float_to_int_result();
        self.fold_structured_void_early_return_branches();
        self.schedule_loop_assertion_entry_alias();
        self.schedule_loop_assertion_string_highs();
        self.schedule_loop_assertion_body();
        if let Some(return_expression) = function.return_expression.as_ref() {
            let result = match function.return_type {
                Type::Float | Type::Double => Eabi::float_result().number,
                _ => Eabi::general_result().number,
            };
            if self.behavior.frame_convention == FrameConvention::LinkageFirst
                && in_place_call_combined_return_name(function).is_some()
                && matches!(return_expression, Expression::Variable(_))
            {
                let Expression::Variable(name) = return_expression else {
                    unreachable!("matched variable return")
                };
                let source = self.general_register_of(name)?;
                self.output.instructions.push(Instruction::AddImmediate {
                    d: result,
                    a: source,
                    immediate: 0,
                });
            } else {
                self.evaluate(return_expression, function.return_type, result)?;
            }
        }
        let lowered_accumulator_return =
            !call_accumulators.is_empty() && self.lower_structured_call_accumulator_return();
        let epilogue = self.output.instructions.len();
        resolve_structured_epilogue_branches(&mut self.output.instructions, epilogue);
        self.fold_branch_into_adjacent_structured_epilogue(epilogue);
        self.fold_adjacent_structured_epilogue_branches();
        // This pass can insert a delayed saved-home copy into the entry
        // prefix. Run it after the durable epilogue placeholders have been
        // resolved; its general instruction-index helper owns finalized branch
        // destinations from here onward.
        self.schedule_entry_member_saved_home(function);
        self.schedule_saved_parameter_derived_initializer();
        self.schedule_post_call_jump_state_reset();
        self.schedule_guarded_saved_receiver_float_call();
        self.schedule_inline_float_pair_final_call();
        self.schedule_inlined_member_address_receiver();
        self.schedule_inlined_store_receiver();
        self.schedule_guarded_mutating_inline(function);
        self.schedule_unused_array_mutating_inline(function);
        self.schedule_unused_array_call_entry(function);
        self.schedule_unused_array_state_entry(function);
        self.schedule_exclusive_inline_arms(function);
        self.schedule_guarded_effect_spawn(function);
        if guarded_structured_constant_return {
            self.schedule_guarded_aggregate_result_compare();
            self.schedule_guarded_inline_float_compare();
        }
        // Each source-level `if` creates a pair of optimizer labels even when
        // both collapse to direct instruction offsets. An explicit `else`
        // contributes its additional arm label. Build 163 exposes those
        // otherwise-hidden labels through the later unwind-symbol ordinal.
        let structured_labels =
            structured_hidden_label_count(&structured_function.statements);
        let frame_prefix_labels = pre_constant_label_count(
            frame_arrays.len(),
            &frame_scalar_locals,
            &function.statements,
            self.inline_statement_body_substitutions,
        );
        if aggregate_call_copy_plan.is_some() {
            // Declaration-time aggregate images are pooled before the body
            // creates its branch labels. Those labels still precede unwind and
            // later-function ordinals, but must not renumber these constants.
            self.output.post_constant_label_bump += structured_labels;
        } else {
            self.output.anonymous_label_bump += structured_labels + frame_prefix_labels;
        }
        if !call_accumulators.is_empty() {
            // Each normalized call result leaves one optimizer-only label. The
            // modern branchless terminal select consumes two more labels even
            // though neither survives into the scheduled instruction stream.
            self.output.anonymous_label_bump += call_accumulator_assignment_count(function);
            if lowered_accumulator_return
                && self.behavior.frame_convention == FrameConvention::Predecrement
            {
                self.output.anonymous_label_bump += 2;
            }
        }
        if dense_inline_save || pooled_dense_inline_save {
            self.output.instructions.extend([
                Instruction::LoadMultipleWord {
                    d: frame_first_saved as u8,
                    a: 1,
                    offset: plan.frame_size - 4 * frame_saved_count as i16,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: plan.frame_size + 4,
                },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: plan.frame_size,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]);
        } else if dense_save_helper {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 11,
                a: 1,
                immediate: plan.frame_size,
            });
            let helper = format!("_restgpr_{frame_first_saved}");
            self.record_relocation(RelocationKind::Rel24, &helper);
            self.output
                .instructions
                .push(Instruction::BranchAndLink { target: helper });
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: plan.frame_size + 4,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: plan.frame_size,
                },
                Instruction::BranchToLinkRegister,
            ]);
        } else {
            self.emit_epilogue_and_return();
        }
        if let Some(layout) = &async_callback_switch_layout {
            self.schedule_async_callback_switch(layout.homes(&homes));
        }
        if pooled_dense_inline_save {
            self.schedule_structured_array_pool_epilogue();
        }
        self.schedule_saved_return_epilogue(&structured_switch_source);
        self.schedule_saved_receiver_entry_epilogue();
        self.schedule_legacy_inline_expansion_residue();
        self.schedule_structured_initializer_live_in();
        self.schedule_structured_member_bound_call();
        if rounded_pointer_dense_layout {
            self.schedule_power_pc_7400_rounded_pointer_entry();
        }
        if dense_frame {
            self.coalesce_member_xor_call_argument_loads();
        }
        if dense_frame && self.behavior.power_pc_7400_scheduling_enabled() {
            self.schedule_power_pc_7400_call_result_handoff(first_saved as u8);
        }
        if rounded_pointer_dense_layout {
            self.schedule_power_pc_7400_rounded_pointer_body();
        }
        if aggregate_call_copy_plan.is_some() {
            // This specialized path emits the outer condition call directly,
            // while nested expressions pass through the ordinary evaluator.
            // The evaluator's partial discovery stream therefore omits the
            // first callee and prevents the pipeline's empty-order fallback.
            // Rebuild the complete source traversal here; symbol creation is
            // independent of the call scheduler used to obtain exact text.
            self.output.symbol_order = crate::symbol_order::referenced_names(
                function,
                &self.call_return_types,
                self.behavior.symbol_traversal_style,
            );
        }
        self.preserve_guarded_named_local_values = variadic_output_frame.is_some();
        if repeated_call_poll_transaction {
            self.structured_cfg_cleanup_owner = true;
            self.structured_repeated_call_poll_owner = true;
        }
        Ok(true)
    }

    pub(super) fn emit_structured_statements(
        &mut self,
        statements: &[Statement],
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        release_dead_float_locations: bool,
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        // An early-return guard has no join from its call-making arm. Preserve
        // condition values only along that guard's fallthrough edge, then let
        // the next condition retain the intersection it also reads.
        let shared_switch_global_plan = plan_structured_shared_switch_global_value(
            statements,
            &self.globals,
            &self.volatile_globals,
        );
        let mut shared_switch_global_restore = None;
        let mut carried_condition_cache_restore = None;
        let mut scheduled_float_store = None;
        for (statement_index, statement) in statements.iter().enumerate() {
            if shared_switch_global_plan
                .as_ref()
                .is_some_and(|plan| plan.activation_index == statement_index)
            {
                let plan = shared_switch_global_plan
                    .as_ref()
                    .expect("activation index came from a plan");
                let previous_cache = std::mem::take(
                    &mut self.condition_global_values,
                );
                match plan.home {
                    SharedSwitchGlobalValueHome::LazyPreferred(register) => {
                        self.condition_global_values.insert(
                            plan.global.clone(),
                            crate::condition_global_cache::ConditionGlobalValue::
                                PendingPreferred(register),
                        );
                    }
                    SharedSwitchGlobalValueHome::EagerFixed(register) => {
                        self.emit_global_load_value(&plan.global, register)?;
                        self.condition_global_values.insert(
                            plan.global.clone(),
                            crate::condition_global_cache::ConditionGlobalValue::
                                Register(register),
                        );
                    }
                }
                let previous_shared =
                    self.structured_shared_switch_global_value.take();
                shared_switch_global_restore =
                    Some((previous_cache, previous_shared));
            }
            let repeats_previous_scratch_constant = statement_index
                .checked_sub(1)
                .and_then(|start| statements.get(start..=statement_index))
                .is_some_and(|pair| {
                    matches!(
                        self.constant_store_run_plan(pair),
                        Some(ConstStoreRun::AllSame)
                    )
                });
            let repeats_next_scratch_constant = statement_index
                .checked_add(1)
                .and_then(|end| statements.get(statement_index..=end))
                .is_some_and(|pair| {
                    matches!(
                        self.constant_store_run_plan(pair),
                        Some(ConstStoreRun::AllSame)
                    )
                });
            let repeated_scratch_constant =
                repeats_previous_scratch_constant || repeats_next_scratch_constant;
            if repeated_scratch_constant {
                if !self.reuse_scratch_constant {
                    self.scratch_constant = None;
                }
                self.reuse_scratch_constant = true;
            } else {
                self.reuse_scratch_constant = false;
                self.scratch_constant = None;
            }
            let emitted_start = self.output.instructions.len();
            match statement {
                _ if self.try_emit_structured_global_self_member_handoff(
                    statement,
                    statements.get(statement_index + 1),
                )? => {}
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    if is_sparse_shared_body_switch(arms) {
                        self.emit_structured_sparse_switch(
                            scrutinee,
                            arms,
                            default.as_ref(),
                            function,
                            ephemeral_locals,
                            return_branches,
                            label_positions,
                            pending_gotos,
                            entry_alias,
                        )?;
                    } else if shared_base_comparison_switch(arms).is_some() {
                        self.emit_structured_comparison_switch(
                            scrutinee,
                            arms,
                            default.as_ref(),
                            function,
                            ephemeral_locals,
                            return_branches,
                            label_positions,
                            pending_gotos,
                            entry_alias,
                        )?;
                    } else {
                        self.emit_structured_dense_switch(
                            scrutinee,
                            arms,
                            default.as_ref(),
                            function,
                            ephemeral_locals,
                            return_branches,
                            label_positions,
                            pending_gotos,
                            entry_alias,
                        )?;
                    }
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if !else_body.is_empty() => self.emit_structured_if_else(
                    condition,
                    then_body,
                    else_body,
                    statement_index,
                    function,
                    ephemeral_locals,
                    return_branches,
                    label_positions,
                    pending_gotos,
                    entry_alias,
                )?,
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if else_body.is_empty() => {
                    if let Some(value) = constant_value(condition) {
                        if value != 0 {
                            self.emit_structured_statements(
                                then_body,
                                function,
                                ephemeral_locals,
                                false,
                                return_branches,
                                label_positions,
                                pending_gotos,
                                entry_alias,
                            )
                            .map_err(|mut diagnostic| {
                                diagnostic.message.push_str(&format!(
                                    " (inside constant structured if statement {statement_index})"
                                ));
                                diagnostic
                            })?;
                        }
                        continue;
                    }
                    if is_lowered_switch_guard(condition) {
                        // A source switch retains its dispatch edge even for a
                        // single case. Branch into the matching arm and use an
                        // explicit fallthrough jump around it; this preserves
                        // switch CFG identity after normalization to an if tree.
                        let (options, condition_bit) =
                            self.emit_condition_test(condition)?;
                        let enter_body = self.output.instructions.len();
                        self.output.instructions.push(
                            Instruction::BranchConditionalForward {
                                options: options ^ 8,
                                condition_bit,
                                target: 0,
                            },
                        );
                        let skip_body = self.output.instructions.len();
                        self.output
                            .instructions
                            .push(Instruction::Branch { target: 0 });
                        self.patch_forward(
                            enter_body,
                            self.output.instructions.len(),
                        );
                        self.emit_structured_statements(
                            then_body,
                            function,
                            ephemeral_locals,
                            false,
                            return_branches,
                            label_positions,
                            pending_gotos,
                            entry_alias,
                        )
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (inside structured switch arm {statement_index})"
                            ));
                            diagnostic
                        })?;
                        let join = self.output.instructions.len();
                        if let Instruction::Branch { target } =
                            &mut self.output.instructions[skip_body]
                        {
                            *target = structured_switch_join_placeholder(join);
                        }
                        continue;
                    }
                    if self.try_emit_guarded_indexed_indirect_call(
                        condition,
                        then_body,
                    )? {
                        continue;
                    }
                    if self.try_emit_guarded_shared_global_member_call(
                        condition,
                        then_body,
                    )? {
                        continue;
                    }
                    if self.try_emit_structured_tail_result_guard(
                        condition,
                        then_body,
                        function,
                    )? {
                        continue;
                    }
                    let previous_wide_mask_cache =
                        self.begin_wide_pair_mask_condition(condition);
                    let nested_condition = match then_body.first() {
                        Some(Statement::If { condition, .. }) => Some(condition),
                        _ => None,
                    };
                    let guarded_store_value = match then_body.first() {
                        Some(Statement::Store { value, .. })
                            if crate::condition_float_cache::is_retained_float_expression(value) =>
                        {
                            Some(value)
                        }
                        _ => None,
                    };
                    let guarded_value_followup =
                        (self.inline_statement_body_substitutions != 0)
                            .then(|| match then_body.first() {
                                Some(Statement::Assign { value, .. }) => Some(value),
                                Some(Statement::Store { target, .. }) => Some(target),
                                Some(Statement::Expression(expression)) => Some(expression),
                                _ => None,
                            })
                            .flatten();
                    let guarded_followup =
                        guarded_store_value
                            .or(nested_condition)
                            .or(guarded_value_followup);
                    let joined_followup = guarded_followup.is_none().then(|| {
                        followup_after_call_free_join(
                            then_body,
                            statements.get(statement_index + 1),
                        )
                    }).flatten();
                    let cache_followup = guarded_followup.or(joined_followup);
                    let terms = logical_and_terms(condition);
                    let (previous_cache, previous_float_cache) =
                        if let Some((previous, previous_float)) =
                            carried_condition_cache_restore.take()
                        {
                            self.continue_condition_global_cache(condition);
                            self.continue_condition_float_cache(condition);
                            (previous, previous_float)
                        } else {
                            (
                                self.begin_condition_global_cache_with_followup(
                                    condition,
                                    cache_followup,
                                ),
                                if let Some(value) = guarded_store_value {
                                    self.begin_composed_condition_float_cache_with_value_followup(
                                        condition,
                                        value,
                                    )
                                } else {
                                    self.begin_composed_condition_float_cache_with_followup(
                                        condition,
                                        nested_condition,
                                    )
                                },
                            )
                        };
                    let previous_member_cache =
                        self.begin_condition_member_cache(condition);
                    let or_plan = logical_or_plan(condition);
                    struct ConditionBranches {
                        skip_body: Vec<usize>,
                        enter_body: Vec<usize>,
                        grouped_equality: bool,
                    }
                    self.preload_condition_literal_reused_in_body(condition, then_body);
                    let condition_result = (|| {
                        if let Some(plan) = or_plan.as_ref().filter(|plan| !plan.suffix.is_empty()) {
                            for term in plan
                                .prefix
                                .iter()
                                .copied()
                                .chain(plan.groups.iter().flatten().copied())
                            {
                                self.preload_condition_global_cache(term)?;
                            }
                        } else {
                            self.preload_condition_global_cache(condition)?;
                        }
                        if entry_alias.is_none() {
                            if let Some((enter_body, skip_body)) = self
                                .try_emit_logical_equality_alternative_branches(condition)?
                            {
                                return Ok(ConditionBranches {
                                    skip_body,
                                    enter_body,
                                    grouped_equality: true,
                                });
                            }
                        }
                        if let Some(or_plan) = or_plan {
                            let mut skip_body = Vec::new();
                            let mut enter_body = Vec::new();
                            for (term_index, term) in
                                or_plan.prefix.iter().copied().enumerate()
                            {
                                let term_start = self.output.instructions.len();
                                let (options, condition_bit) = self
                                    .emit_condition_test(term)
                                    .map_err(|mut diagnostic| {
                                        diagnostic.message.push_str(&format!(
                                            " (in structured if condition {statement_index})"
                                        ));
                                        diagnostic
                                    })?;
                                self.reuse_short_circuit_member_base(term_index, term_start);
                                if statement_index == 0 && term_index == 0 {
                                    if let Some(alias) = entry_alias.as_ref() {
                                        fold_entry_alias_zero_test(
                                            &mut self.output.instructions,
                                            alias,
                                        );
                                    }
                                }
                                skip_body.push(self.output.instructions.len());
                                self.output.instructions.push(
                                    Instruction::BranchConditionalForward {
                                        options,
                                        condition_bit,
                                        target: 0,
                                    },
                                );
                                if statement_index == 0 && term_index == 0 {
                                    if let Some(alias) = entry_alias.take() {
                                        self.locations
                                            .get_mut(&alias.name)
                                            .expect("planned saved parameter")
                                            .register = alias.home;
                                    }
                                }
                            }
                            for (group_index, group) in or_plan.groups.iter().enumerate() {
                                let last_group = group_index + 1 == or_plan.groups.len();
                                let mut advance_group = Vec::new();
                                let mut next_group_float_cache = None;
                                for (term_index, term) in group.iter().copied().enumerate() {
                                    let term_start = self.output.instructions.len();
                                    let (options, condition_bit) = self
                                        .emit_condition_test(term)
                                        .map_err(|mut diagnostic| {
                                            diagnostic.message.push_str(&format!(
                                                " (in structured if condition {statement_index})"
                                            ));
                                            diagnostic
                                        })?;
                                    self.reuse_short_circuit_member_base(term_index, term_start);
                                    if or_plan.prefix.is_empty()
                                        && group_index == 0
                                        && term_index == 0
                                    {
                                        if let Some(alias) = entry_alias.as_ref() {
                                            fold_entry_alias_zero_test(
                                                &mut self.output.instructions,
                                                alias,
                                            );
                                        }
                                    }
                                    if !last_group && term_index == 0 {
                                        // Any failed term advances to the next OR group.
                                        // Only values established by the first term dominate
                                        // every one of those incoming edges; values first loaded
                                        // by later terms must not leak into the next group.
                                        next_group_float_cache =
                                            Some(self.condition_float_cache.clone());
                                    }
                                    let branch = self.output.instructions.len();
                                    if !last_group && term_index + 1 == group.len() {
                                        self.output.instructions.push(
                                            Instruction::BranchConditionalForward {
                                                options: options ^ 8,
                                                condition_bit,
                                                target: 0,
                                            },
                                        );
                                        enter_body.push(branch);
                                    } else {
                                        self.output.instructions.push(
                                            Instruction::BranchConditionalForward {
                                                options,
                                                condition_bit,
                                                target: 0,
                                            },
                                        );
                                        if last_group {
                                            skip_body.push(branch);
                                        } else {
                                            advance_group.push(branch);
                                        }
                                    }
                                    if or_plan.prefix.is_empty()
                                        && group_index == 0
                                        && term_index == 0
                                    {
                                        if let Some(alias) = entry_alias.take() {
                                            self.locations
                                                .get_mut(&alias.name)
                                                .expect("planned saved parameter")
                                                .register = alias.home;
                                        }
                                    }
                                }
                                let next_group = self.output.instructions.len();
                                for branch in advance_group {
                                    self.patch_forward(branch, next_group);
                                }
                                if let Some(cache) = next_group_float_cache {
                                    self.condition_float_cache = cache;
                                }
                            }
                            if !or_plan.suffix.is_empty() {
                                let suffix_start = self.output.instructions.len();
                                for branch in enter_body.drain(..) {
                                    self.patch_forward(branch, suffix_start);
                                }
                                for term in or_plan.suffix {
                                    let (options, condition_bit) =
                                        self.emit_condition_test(term).map_err(
                                            |mut diagnostic| {
                                                diagnostic.message.push_str(&format!(
                                                    " (in structured if condition {statement_index})"
                                                ));
                                                diagnostic
                                            },
                                        )?;
                                    skip_body.push(self.output.instructions.len());
                                    self.output.instructions.push(
                                        Instruction::BranchConditionalForward {
                                            options,
                                            condition_bit,
                                            target: 0,
                                        },
                                    );
                                }
                            }
                            return Ok(ConditionBranches {
                                skip_body,
                                enter_body,
                                grouped_equality: false,
                            });
                        }
                        let mut branches = Vec::with_capacity(terms.len());
                        for (term_index, term) in terms.iter().copied().enumerate() {
                            let term_start = self.output.instructions.len();
                            let retained_assertion_condition = if term_index == 0 {
                                self.emit_leading_inline_assertion(term)?
                            } else {
                                self.emit_proven_inline_assertion(terms[term_index - 1], term)?
                            };
                            let (options, condition_bit) = match retained_assertion_condition {
                                Some(condition) => condition,
                                None => {
                                    self.emit_condition_test(term).map_err(|mut diagnostic| {
                                        diagnostic.message.push_str(&format!(
                                            " (in structured if condition {statement_index})"
                                        ));
                                        diagnostic
                                    })?
                                }
                            };
                            self.reuse_short_circuit_member_base(term_index, term_start);
                            if statement_index == 0 && term_index == 0 {
                                if let Some(alias) = entry_alias.as_ref() {
                                    fold_entry_alias_zero_test(
                                        &mut self.output.instructions,
                                        alias,
                                    );
                                }
                            }
                            branches.push(self.output.instructions.len());
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options,
                                    condition_bit,
                                    target: 0,
                                });
                            if statement_index == 0 && term_index == 0 {
                                if let Some(alias) = entry_alias.take() {
                                    self.locations
                                        .get_mut(&alias.name)
                                        .expect("planned saved parameter")
                                        .register = alias.home;
                                }
                            }
                        }
                        Ok(ConditionBranches {
                            skip_body: branches,
                            enter_body: Vec::new(),
                            grouped_equality: false,
                        })
                    })();
                    self.restore_condition_member_cache(previous_member_cache);
                    let carry_fallthrough_cache = matches!(
                        then_body.last(),
                        Some(Statement::Return(None) | Statement::Goto(_))
                    ) && matches!(
                        statements.get(statement_index + 1),
                        Some(Statement::If { else_body, .. }) if else_body.is_empty()
                    );
                    let continuation_cache = if carry_fallthrough_cache {
                        Some((
                            self.condition_global_values.clone(),
                            self.condition_float_cache.clone(),
                        ))
                    } else if joined_followup.is_some() {
                        retained_values_after_join(
                            self.condition_global_values.clone(),
                            then_body,
                        )
                        .map(|values| (values, Default::default()))
                    } else {
                        None
                    };
                    let guarded_true_cache =
                        guarded_followup.map(|_| self.condition_global_values.clone());
                    let guarded_true_float_cache = guarded_followup.map(|followup| {
                        self.condition_float_true_edge_cache(followup)
                    });
                    let then_wide_mask_cache = self.wide_pair_mask_false_edge_cache();
                    let then_literal_cache = self.condition_float_literal_edge_cache();
                    self.restore_condition_global_cache(previous_cache);
                    let branches = match condition_result {
                        Ok(branches) => branches,
                        Err(diagnostic) => {
                            self.restore_condition_float_cache(previous_float_cache);
                            self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
                            return Err(diagnostic);
                        }
                    };
                    let ConditionBranches {
                        mut skip_body,
                        enter_body,
                        grouped_equality,
                    } = branches;
                    self.commit_structured_float_handoff();
                    let body_start = self.output.instructions.len();
                    for &branch in &enter_body {
                        self.patch_forward(branch, body_start);
                    }
                    // Only the proven first guarded statement may consume
                    // true-edge values. Restore before any subsequent source
                    // statement can mutate the referenced memory.
                    let carried_prefix_len = guarded_followup.is_some() as usize;
                    let (carried_prefix, remainder) =
                        then_body.split_at(carried_prefix_len);
                    let prefix_cache_restore = guarded_true_cache.map(|cache| {
                        std::mem::replace(&mut self.condition_global_values, cache)
                    });
                    if let Some(cache) = guarded_true_float_cache {
                        self.condition_float_cache = cache;
                    }
                    self.wide_pair_mask_cache = then_wide_mask_cache;
                    let prefix_result = self.emit_structured_statements(
                        carried_prefix,
                        function,
                        ephemeral_locals,
                        false,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    );
                    if let Some(previous) = prefix_cache_restore {
                        self.restore_condition_global_cache(previous);
                    }
                    self.restore_condition_float_cache(previous_float_cache);
                    let outer_float_cache = std::mem::replace(
                        &mut self.condition_float_cache,
                        then_literal_cache,
                    );
                    let body_result = prefix_result.and_then(|()| {
                        self.emit_structured_statements(
                            remainder,
                            function,
                            ephemeral_locals,
                            false,
                            return_branches,
                            label_positions,
                            pending_gotos,
                            entry_alias,
                        )
                    });
                    self.restore_condition_float_cache(outer_float_cache);
                    let body_result = body_result.map_err(|mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (inside structured if statement {statement_index})"
                        ));
                        diagnostic
                    });
                    self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
                    body_result?;
                    if grouped_equality {
                        self.fold_logical_equality_alternative_goto(
                            then_body,
                            body_start,
                            &enter_body,
                            &mut skip_body,
                            pending_gotos,
                        );
                    }
                    let target = self.output.instructions.len();
                    for branch in skip_body {
                        if let Instruction::BranchConditionalForward {
                            target: branch_target,
                            ..
                        } = &mut self.output.instructions[branch]
                        {
                            *branch_target = target;
                        }
                    }
                    if let Some((cache, float_cache)) = continuation_cache {
                        let previous = std::mem::replace(&mut self.condition_global_values, cache);
                        let previous_float =
                            std::mem::replace(&mut self.condition_float_cache, float_cache);
                        carried_condition_cache_restore = Some((previous, previous_float));
                    }
                }
                Statement::Return(Some(value)) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output
                        .instructions
                        .push(Instruction::Branch {
                            target: STRUCTURED_EPILOGUE_PLACEHOLDER,
                        });
                }
                Statement::Return(None) => {
                    return_branches.push(self.output.instructions.len());
                    self.output
                        .instructions
                        .push(Instruction::Branch {
                            target: STRUCTURED_EPILOGUE_PLACEHOLDER,
                        });
                }
                Statement::Goto(label) => {
                    let branch = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::Branch { target: 0 });
                    pending_gotos.push((branch, label.clone()));
                }
                Statement::Label(label) => {
                    if label_positions
                        .insert(label.clone(), self.output.instructions.len())
                        .is_some()
                    {
                        return Err(Diagnostic::error(format!(
                            "structured body defines label '{label}' more than once"
                        )));
                    }
                }
                Statement::Assign { name, value } => {
                    if name.starts_with("__mwcc_iterator_end_") {
                        self.emit_loop_assertion_string_highs();
                    }
                    if is_unobserved_local_assignment(function, name)
                        && !crate::analysis::expression_has_side_effect(value)
                        && self.volatile_globals.iter().all(|global| {
                            !crate::analysis::expression_reads_name(value, global)
                        })
                    {
                        continue;
                    }
                    if is_folded_terminal_pointer_load_alias(function, statement_index) {
                        continue;
                    }
                    if self.try_emit_frame_aggregate_call_assignment(name, value)? {
                        continue;
                    }
                    if self.frame_slots.contains_key(name) {
                        self.emit_store(&Expression::Variable(name.clone()), value)
                            .map_err(|mut diagnostic| {
                                diagnostic.message.push_str(&format!(
                                    " (in structured frame assignment statement {statement_index}, target '{name}', value {value:?})"
                                ));
                                diagnostic
                            })?;
                        continue;
                    }
                    let declared_type = function
                        .locals
                        .iter()
                        .find(|local| &local.name == name)
                        .map(|local| local.declared_type)
                        .or_else(|| {
                            function
                                .parameters
                                .iter()
                                .find(|parameter| &parameter.name == name)
                                .map(|parameter| parameter.parameter_type)
                        })
                        .expect("eligibility checked");
                    let previous = self.locations.get(name).map(|location| location.register);
                    let remaining = &statements[statement_index + 1..];
                    // The source-level return is emitted after every statement, but is not
                    // part of `remaining`.  A value tested before a later call and returned
                    // afterward is therefore not terminal in a volatile register: the call
                    // result must stay in its planned callee-saved home through that call.
                    let returned_after_later_call = function
                        .return_expression
                        .as_ref()
                        .is_some_and(|expression| expression_reads_name(expression, name))
                        && remaining.iter().any(statement_has_call);
                    let terminal_volatile = matches!(declared_type, Type::Int | Type::UnsignedInt)
                        && value_read_before_redefinition(remaining, name)
                        && !read_after_possible_call(remaining, name, false).read_after_call
                        && !returned_after_later_call
                        && !self.inline_source_call_survivors.contains(name);
                    if terminal_volatile && matches!(value, Expression::Call { .. }) {
                        self.evaluate(value, declared_type, Eabi::general_result().number)?;
                        self.locations
                            .get_mut(name)
                            .expect("structured assignment home")
                            .register = Eabi::general_result().number;
                        continue;
                    }
                    if terminal_volatile {
                        if let Expression::Variable(source) = value {
                            if let Some(source) = self.lookup_general(source) {
                                self.locations
                                    .get_mut(name)
                                    .expect("structured assignment home")
                                    .register = source;
                                continue;
                            }
                        }
                    }
                    let preference = previous
                        .and_then(|register| {
                            mwcc_vreg::Reg::from_field(register, mwcc_vreg::Class::General)
                                .virtual_register()
                        })
                        .and_then(|register| self.register_prefer.get(&register).copied());
                    let dying_preference = preference.or_else(|| {
                        function
                            .locals
                            .iter()
                            .map(|local| local.name.as_str())
                            .chain(
                                function
                                    .parameters
                                    .iter()
                                    .map(|parameter| parameter.name.as_str()),
                            )
                            .filter(|candidate| *candidate != name)
                            .find_map(|candidate| {
                                (expression_reads_name(value, candidate)
                                    && !body_uses_local(
                                        &statements[statement_index + 1..],
                                        candidate,
                                    ))
                                .then(|| self.locations.get(candidate))
                                .flatten()
                                .and_then(|location| {
                                    mwcc_vreg::Reg::from_field(
                                        location.register,
                                        mwcc_vreg::Class::General,
                                    )
                                    .virtual_register()
                                })
                                .and_then(|register| {
                                    self.register_prefer.get(&register).copied()
                                })
                            })
                    });
                    let accumulator = self.try_emit_structured_call_accumulator(
                        name,
                        value,
                        previous,
                        dying_preference,
                    )?;
                    if let Some(destination) = accumulator {
                        self.locations.insert(
                            name.clone(),
                            Location {
                                class: ValueClass::General,
                                register: destination,
                                signed: self.signed_of(declared_type),
                                width: declared_type.width(),
                                pointee: None,
                                stride: None,
                            },
                        );
                    } else {
                        let previous = previous.unwrap_or_else(|| {
                            let version_preference = has_split_value_version(function, name)
                                .then(|| {
                                    32usize
                                        .checked_sub(self.callee_saved.len())?
                                        .checked_add(1)
                                        .and_then(|register| u8::try_from(register).ok())
                                })
                                .flatten();
                            let register = if let Some(preferred) = version_preference {
                                self.fresh_virtual_general_preferring(preferred)
                            } else {
                                self.fresh_virtual_general()
                            };
                            self.locations.insert(
                                name.clone(),
                                Location {
                                    class: ValueClass::General,
                                    register,
                                    signed: self.signed_of(declared_type),
                                    width: declared_type.width(),
                                    pointee: match declared_type {
                                        Type::Pointer(pointee) => Some(pointee),
                                        _ => None,
                                    },
                                    stride: pointer_stride(declared_type),
                                },
                            );
                            register
                        });
                        let terminal_result = self.behavior.frame_convention
                            == FrameConvention::Predecrement
                            && statement_index + 1 == statements.len()
                            && in_place_call_combined_return_name(function) == Some(name.as_str());
                        let separates_live_alias = reassignment_live_source(
                            function,
                            name,
                            value,
                            &statements[statement_index + 1..],
                        )
                        .and_then(|source| self.locations.get(source))
                        .is_some_and(|source| source.register == previous);
                        let terminal_argument = terminal_volatile
                            .then(|| {
                                terminal_offset_call_argument_register(
                                    value,
                                    statements.get(statement_index + 1),
                                    name,
                                )
                            })
                            .flatten();
                        let split_leaf_parameter_mask =
                            !self.non_leaf && leaf_parameter_mask_version(function, name, value);
                        let destination = if terminal_result {
                            Eabi::general_result().number
                        } else if let Some(register) = terminal_argument {
                            register
                        } else if split_leaf_parameter_mask {
                            self.fresh_virtual_general_preferring(4)
                        } else if separates_live_alias {
                            if let Some(register) = transient_call_argument_register(
                                &statements[statement_index + 1..],
                                name,
                            ) {
                                self.fresh_virtual_general_preferring(register)
                            } else {
                                self.fresh_virtual_general()
                            }
                        } else {
                            previous
                        };
                        let handled_wide_initializer =
                            self.try_emit_structured_wide_saved_initializer(value, destination);
                        let handled_call_combine = !handled_wide_initializer
                            && self.try_emit_structured_in_place_call_combine(
                                name,
                                value,
                                destination,
                            )?;
                        let handled_computed_address = if !handled_wide_initializer
                            && !handled_call_combine
                        {
                            if let (
                                Type::StructPointer { element_size },
                                Expression::AddressOf { operand },
                            ) = (declared_type, value)
                            {
                                if let Expression::Index { base, index } = operand.as_ref() {
                                    if let (
                                        Expression::Variable(global),
                                        Expression::Variable(index_name),
                                    ) = (base.as_ref(), index.as_ref())
                                    {
                                        if self.global_array_sizes.contains_key(global) {
                                            let index_register = self.lookup_general(index_name).ok_or_else(|| {
                                            Diagnostic::error("structured computed address index has no register")
                                        })?;
                                            let retained_element = self
                                                .structured_global_index_cache
                                                .as_ref()
                                                .filter(|cache| {
                                                    cache.global == *global
                                                        && cache.index == *index_name
                                                        && cache.stride == element_size
                                                })
                                                .and_then(|cache| {
                                                    cache.retained_element.map(|retained| {
                                                        (
                                                            retained,
                                                            cache.retained_element_initialized,
                                                        )
                                                    })
                                                });
                                            if let Some((retained, true)) = retained_element {
                                                self.locations
                                                    .get_mut(name)
                                                    .expect("structured computed address local")
                                                    .register = retained;
                                                true
                                            } else {
                                                let high = self.fresh_virtual_general();
                                                let cached_scaled = self
                                                    .structured_global_index_cache
                                                    .as_ref()
                                                    .filter(|cache| {
                                                        cache.global == *global
                                                            && cache.index == *index_name
                                                            && cache.stride == element_size
                                                    })
                                                    .map(|cache| cache.scaled);
                                                let scaled = cached_scaled.unwrap_or_else(|| {
                                                    self.fresh_virtual_general()
                                                });
                                                self.emit_address_high(high, global);
                                                if cached_scaled.is_none() {
                                                    emit_scaled_index(
                                                        &mut self.output.instructions,
                                                        scaled,
                                                        index_register,
                                                        element_size,
                                                    )?;
                                                }
                                                self.record_relocation(
                                                    RelocationKind::Addr16Lo,
                                                    global,
                                                );
                                                self.output.instructions.push(
                                                    Instruction::AddImmediate {
                                                        d: GENERAL_SCRATCH,
                                                        a: high,
                                                        immediate: 0,
                                                    },
                                                );
                                                let computed_destination =
                                                    retained_element.map_or(
                                                        destination,
                                                        |(retained, _)| retained,
                                                    );
                                                self.output.instructions.push(Instruction::Add {
                                                    d: computed_destination,
                                                    a: GENERAL_SCRATCH,
                                                    b: scaled,
                                                });
                                                if retained_element.is_some() {
                                                    self.structured_global_index_cache
                                                        .as_mut()
                                                        .expect(
                                                            "retained global-index cache disappeared",
                                                        )
                                                        .retained_element_initialized = true;
                                                    self.locations
                                                        .get_mut(name)
                                                        .expect(
                                                            "structured computed address local",
                                                        )
                                                        .register = computed_destination;
                                                }
                                                true
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if handled_wide_initializer
                            || handled_call_combine
                            || handled_computed_address
                        {
                            Ok(())
                        } else {
                            // A passthrough call argument can keep a physical
                            // entry/result register live without emitting an
                            // instruction that exposes that liveness to the
                            // allocator. Reserve such homes while selecting a
                            // side-effecting postfix RHS so its old-value
                            // temporary cannot silently overwrite them.
                            let reserved_live_homes = matches!(value, Expression::PostStep { .. })
                                .then(|| {
                                    function
                                        .locals
                                        .iter()
                                        .map(|local| local.name.as_str())
                                        .chain(
                                            function
                                                .parameters
                                                .iter()
                                                .map(|parameter| parameter.name.as_str()),
                                        )
                                        .filter(|candidate| *candidate != name)
                                        .filter(|candidate| {
                                            value_read_before_redefinition(remaining, candidate)
                                        })
                                        .filter_map(|candidate| self.locations.get(candidate))
                                        .filter_map(|location| {
                                            (!mwcc_vreg::Reg::is_virtual_field(location.register))
                                                .then_some(location.register)
                                        })
                                        .filter(|register| self.reserved.insert(*register))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            let packed_minimum = self.packed_shift_mask_min_operations;
                            if name.starts_with("__mwcc_packet_word_") {
                                self.packed_shift_mask_min_operations = 2;
                            }
                            let result = self.evaluate_register_store_value(
                                value,
                                declared_type,
                                destination,
                            );
                            self.packed_shift_mask_min_operations = packed_minimum;
                            for register in reserved_live_homes {
                                self.reserved.remove(&register);
                            }
                            result
                        }
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (in structured assignment statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                        self.locations
                            .get_mut(name)
                            .expect("structured assignment home")
                            .width = assigned_register_width(declared_type, value);
                        if terminal_result
                            || separates_live_alias
                            || terminal_argument.is_some()
                            || split_leaf_parameter_mask
                        {
                            self.locations
                                .get_mut(name)
                                .expect("structured assignment home")
                                .register = destination;
                        }
                    }
                }
                Statement::Loop { .. } => {
                    if !self.try_emit_global_struct_member_search_loop_in_function(
                        statement,
                        Some(function),
                    )? {
                        return Err(Diagnostic::error(
                            "structured loop has no matching semantic owner",
                        ));
                    }
                }
                Statement::Expression(expression @ Expression::Conditional { .. }) => {
                    if !self.try_emit_conditional_call_statement(expression)? {
                        self.emit_comma_side_effect(expression).map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (in structured side-effect statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                    }
                }
                Statement::Expression(
                    expression @ (Expression::Comma { .. } | Expression::Assign { .. }),
                ) => self.emit_comma_side_effect(expression).map_err(|mut diagnostic| {
                    diagnostic.message.push_str(&format!(
                        " (in structured side-effect statement {statement_index})"
                    ));
                    diagnostic
                })?,
                _ => self.emit_statement(statement).map_err(|mut diagnostic| {
                    diagnostic.message.push_str(&format!(
                        " (in structured body statement {statement_index}: {statement:?})"
                    ));
                    diagnostic
                })?,
            }
            self.stage_legacy_shift_add_call_argument(
                statement,
                &statements[statement_index + 1..],
                emitted_start,
            );
            self.schedule_dying_structured_local_argument(
                statement,
                &statements[statement_index + 1..],
                function,
                emitted_start,
            );
            self.schedule_saved_receiver_entry_call(
                statement,
                function,
                statement_index,
                emitted_start,
            );
            if let Some(store_index) = scheduled_float_store.take() {
                self.swap_structured_float_store_with_guard_test(store_index)?;
            }
            if self.plans_structured_float_store_guard_swap(
                statement,
                statements.get(statement_index + 1),
            ) {
                scheduled_float_store = self.output.instructions.len().checked_sub(1);
            }
            if release_dead_float_locations {
                self.release_dead_ephemeral_float_locations(
                    ephemeral_locals,
                    &statements[statement_index + 1..],
                );
            }
            if shared_switch_global_plan
                .as_ref()
                .is_some_and(|plan| plan.activation_index == statement_index)
            {
                let plan = shared_switch_global_plan
                    .as_ref()
                    .expect("activation index came from a plan");
                let Some(
                    crate::condition_global_cache::ConditionGlobalValue::
                        Register(register),
                ) = self.condition_global_values.get(&plan.global).copied()
                else {
                    return Err(Diagnostic::error(
                        "planned shared switch global was not materialized",
                    ));
                };
                self.structured_shared_switch_global_value =
                    Some((plan.global.clone(), register));
            }
            if shared_switch_global_plan
                .as_ref()
                .is_some_and(|plan| plan.completion_index == statement_index)
            {
                let (previous_cache, previous_shared) =
                    shared_switch_global_restore.take().expect(
                        "shared switch global scope must have been activated",
                    );
                self.restore_condition_global_cache(previous_cache);
                self.structured_shared_switch_global_value = previous_shared;
            }
        }
        self.reuse_scratch_constant = false;
        self.scratch_constant = None;
        debug_assert!(scheduled_float_store.is_none());
        if let Some((previous, previous_float)) = carried_condition_cache_restore {
            self.restore_condition_global_cache(previous);
            self.restore_condition_float_cache(previous_float);
        }
        Ok(())
    }

    fn release_dead_ephemeral_float_locations(
        &mut self,
        ephemeral_locals: &[&LocalDeclaration],
        remaining_statements: &[Statement],
    ) {
        for name in dead_ephemeral_float_locals(ephemeral_locals, remaining_statements) {
            self.locations.remove(name);
        }
    }
}

fn structured_return_is_supported(function: &Function) -> bool {
    (function.return_type == Type::Void && function.return_expression.is_none())
        || (matches!(
            function.return_type,
            Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. }
                | Type::Float
                | Type::Double
        ) && (function.return_expression.is_some()
            || statements_always_return(&function.statements)))
}

/// Whether control cannot reach the end of this statement sequence. Structured
/// lowering already emits source-level returns through a shared epilogue; an
/// integer function whose final if/else tree returns from every leaf therefore
/// needs no synthetic trailing return expression.
fn statements_always_return(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Return(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            !else_body.is_empty()
                && statements_always_return(then_body)
                && statements_always_return(else_body)
        }
        _ => false,
    })
}

fn supports_statements(
    statements: &[Statement],
    function: &Function,
    global_array_sizes: &std::collections::HashMap<String, u32>,
    allow_global_search_loop: bool,
) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::InlineAsm(_)
        | Statement::Store { .. }
        | Statement::Expression(_)
        | Statement::Return(Some(_))
        | Statement::Return(None)
        | Statement::Goto(_)
        | Statement::Label(_) => true,
        Statement::Assign { name, .. } => {
            function.locals.iter().any(|local| &local.name == name)
                || function
                    .parameters
                    .iter()
                    .any(|parameter| &parameter.name == name)
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            supports_statements(
                then_body,
                function,
                global_array_sizes,
                allow_global_search_loop,
            ) && supports_statements(
                else_body,
                function,
                global_array_sizes,
                allow_global_search_loop,
            )
        }
        Statement::Loop { .. } => {
            allow_global_search_loop
                && super::super::global_struct_member_search::is_global_struct_member_search_loop(
                    statement,
                    global_array_sizes,
                )
        }
        _ => false,
    })
}

fn pure_local_alias(local: &LocalDeclaration) -> Option<&str> {
    let mut expression = local.initializer.as_ref()?;
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn value_read_before_redefinition(statements: &[Statement], name: &str) -> bool {
    for statement in statements {
        match statement {
            Statement::InlineAsm(_) => {}
            Statement::Assign {
                name: assigned,
                value,
            } => {
                if expression_reads_name(value, name) {
                    return true;
                }
                if assigned == name {
                    return false;
                }
            }
            Statement::Store { target, value } => {
                if expression_reads_name(target, name) || expression_reads_name(value, name) {
                    return true;
                }
            }
            Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                if expression_reads_name(expression, name) {
                    return true;
                }
            }
            Statement::If { condition, .. } => {
                return expression_reads_name(condition, name);
            }
            Statement::Return(None)
            | Statement::Goto(_)
            | Statement::Break
            | Statement::Continue => return false,
            Statement::Label(_) => {}
            Statement::Loop { .. } | Statement::Switch { .. } => return false,
        }
    }
    false
}

fn is_call_result_local(statements: &[Statement], candidate: &str) -> bool {
    statements.iter().any(|statement| {
        matches!(
            statement,
            Statement::Assign { name, value }
                if name == candidate && expression_ends_in_call(value)
        )
    })
}

fn is_sequenced_call_result_local(statements: &[Statement], candidate: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign {
            name,
            value: value @ Expression::Comma { .. },
        } => name == candidate && expression_ends_in_call(value),
        _ => false,
    })
}

fn expression_ends_in_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. } => true,
        Expression::Comma { right, .. } => expression_ends_in_call(right),
        _ => false,
    }
}

pub(crate) fn structured_hidden_label_count(statements: &[Statement]) -> u32 {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                2 + logical_and_count(condition)
                    + u32::from(!else_body.is_empty())
                    + structured_hidden_label_count(then_body)
                    + structured_hidden_label_count(else_body)
            }
            Statement::Label(label) if label.starts_with("__mwcc_structured_loop_") => 1,
            _ => 0,
        })
        .sum()
}

fn logical_and_count(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } => 1 + logical_and_count(left) + logical_and_count(right),
        _ => 0,
    }
}

fn is_plain_short_circuit_call_if(function: &Function) -> bool {
    function.return_type == Type::Void
        && function.return_expression.is_none()
        && function.locals.is_empty()
        && matches!(
            function.statements.as_slice(),
            [Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    ..
                },
                then_body,
                else_body,
            }] if else_body.is_empty()
                && matches!(then_body.as_slice(), [Statement::Expression(Expression::Call { .. })])
        )
}

fn is_guarded_structured_constant_return(function: &Function) -> bool {
    function
        .return_expression
        .as_ref()
        .and_then(constant_value)
        .is_some()
        && matches!(
            function.statements.first(),
            Some(Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    ..
                },
                ..
            })
        )
}

pub(super) fn logical_and_terms(expression: &Expression) -> Vec<&Expression> {
    let mut terms = Vec::new();
    fn collect<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
        if let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } = expression
        {
            collect(left, terms);
            collect(right, terms);
        } else {
            terms.push(expression);
        }
    }
    collect(expression, &mut terms);
    terms
}

pub(super) struct LogicalOrPlan<'a> {
    pub(super) prefix: Vec<&'a Expression>,
    pub(super) groups: Vec<Vec<&'a Expression>>,
    pub(super) suffix: Vec<&'a Expression>,
}

/// An ordered OR-of-AND plan, optionally guarded by a shared leading
/// or trailing conjunction. Besides a direct `(a && b) || (c && d)`, this
/// recognizes `prefix && ((a && b) || (c && d)) && suffix` without
/// distributing and re-emitting either shared conjunction.
pub(super) fn logical_or_plan(expression: &Expression) -> Option<LogicalOrPlan<'_>> {
    if let Some(groups) = logical_or_groups(expression) {
        return Some(LogicalOrPlan {
            prefix: Vec::new(),
            groups,
            suffix: Vec::new(),
        });
    }
    let terms = logical_and_terms(expression);
    let mut alternatives = terms
        .iter()
        .enumerate()
        .filter_map(|(index, term)| logical_or_groups(term).map(|groups| (index, groups)));
    let (alternative_index, groups) = alternatives.next()?;
    if alternatives.next().is_some() {
        return None;
    }
    Some(LogicalOrPlan {
        prefix: terms[..alternative_index].to_vec(),
        groups,
        suffix: terms[alternative_index + 1..].to_vec(),
    })
}

/// A top-level OR expressed as ordered AND groups. This is the source CFG for
/// conditions such as `(a && b) || (c && d)`: each failed group advances to the
/// next one, while a completed group enters the guarded body directly.
pub(super) fn logical_or_groups(expression: &Expression) -> Option<Vec<Vec<&Expression>>> {
    let Expression::Binary {
        operator: BinaryOperator::LogicalOr,
        ..
    } = expression
    else {
        return None;
    };
    fn collect<'a>(expression: &'a Expression, groups: &mut Vec<Vec<&'a Expression>>) {
        if let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left,
            right,
        } = expression
        {
            collect(left, groups);
            collect(right, groups);
        } else {
            groups.push(logical_and_terms(expression));
        }
    }
    let mut groups = Vec::new();
    collect(expression, &mut groups);
    Some(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposes_a_disjunction_into_ordered_conjunction_groups() {
        let variable = |name: &str| Expression::Variable(name.into());
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(variable("a")),
                right: Box::new(variable("b")),
            }),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(variable("c")),
                right: Box::new(variable("d")),
            }),
        };

        let groups = logical_or_groups(&condition).expect("the top-level OR should decompose");
        assert_eq!(groups.len(), 2);
        assert!(matches!(groups[0].as_slice(), [
            Expression::Variable(a),
            Expression::Variable(b),
        ] if a == "a" && b == "b"));
        assert!(matches!(groups[1].as_slice(), [
            Expression::Variable(c),
            Expression::Variable(d),
        ] if c == "c" && d == "d"));
    }

    #[test]
    fn factors_a_shared_conjunction_before_disjunction_groups() {
        let variable = |name: &str| Expression::Variable(name.into());
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(variable("prefix")),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    left: Box::new(variable("a")),
                    right: Box::new(variable("b")),
                }),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    left: Box::new(variable("c")),
                    right: Box::new(variable("d")),
                }),
            }),
        };

        let plan = logical_or_plan(&condition).expect("the guarded OR should decompose");
        assert!(matches!(
            plan.prefix.as_slice(),
            [Expression::Variable(prefix)] if prefix == "prefix"
        ));
        assert_eq!(plan.groups.len(), 2);
        assert!(plan.suffix.is_empty());
        assert!(matches!(
            plan.groups[0].as_slice(),
            [Expression::Variable(a), Expression::Variable(b)] if a == "a" && b == "b"
        ));
        assert!(matches!(
            plan.groups[1].as_slice(),
            [Expression::Variable(c), Expression::Variable(d)] if c == "c" && d == "d"
        ));
    }

    #[test]
    fn retains_a_shared_conjunction_after_disjunction_groups() {
        let variable = |name: &str| Expression::Variable(name.into());
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: Box::new(variable("a")),
                right: Box::new(variable("b")),
            }),
            right: Box::new(variable("suffix")),
        };

        let plan = logical_or_plan(&condition).expect("the trailing conjunction should decompose");
        assert!(plan.prefix.is_empty());
        assert_eq!(plan.groups.len(), 2);
        assert!(matches!(
            plan.suffix.as_slice(),
            [Expression::Variable(suffix)] if suffix == "suffix"
        ));
    }

    #[test]
    fn recognizes_a_nested_if_tree_that_returns_from_every_leaf() {
        let returned = |value| Statement::Return(Some(Expression::IntegerLiteral(value)));
        let nested = Statement::If {
            condition: Expression::Variable("outer".into()),
            then_body: vec![Statement::If {
                condition: Expression::Variable("inner".into()),
                then_body: vec![returned(1)],
                else_body: vec![returned(2)],
            }],
            else_body: vec![returned(3)],
        };
        assert!(statements_always_return(&[nested]));
    }

    #[test]
    fn counts_the_extra_optimizer_label_for_each_explicit_else_arm() {
        let call = || Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: Vec::new(),
        });
        let nested = Statement::If {
            condition: Expression::Variable("outer".into()),
            then_body: vec![Statement::If {
                condition: Expression::Variable("inner".into()),
                then_body: vec![call()],
                else_body: Vec::new(),
            }],
            else_body: vec![call()],
        };

        assert_eq!(structured_hidden_label_count(&[nested]), 5);
    }

    #[test]
    fn rejects_an_if_tree_with_a_fallthrough_leaf() {
        let incomplete = Statement::If {
            condition: Expression::Variable("condition".into()),
            then_body: vec![Statement::Return(Some(Expression::IntegerLiteral(1)))],
            else_body: Vec::new(),
        };
        assert!(!statements_always_return(&[incomplete]));
    }
}
