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
use super::structured_entry_alias::{
    fold_entry_alias_zero_test, plan_first_call_alias, EntryAliasBoundary, EntryParameterAlias,
};
use super::structured_frame_assignment::{
    adjacent_byte_pointer_round_up_name, fold_adjacent_byte_pointer_round_up,
    is_transient_biased_scaled_member_call_local, is_transient_shifted_member_mask_call_local,
    sink_low_mask_parameter_assignment, sink_single_use_parameter_assignment,
};
use super::structured_frame_entry::structured_dense_frame_entry_index;
use super::structured_liveness::read_after_possible_call;
use super::structured_locals::{
    body_uses_local, dead_ephemeral_float_locals, is_definitely_assigned_before_reads,
    plan_deferred_saved_homes, plan_ephemeral_locals,
};
use super::structured_prologue::saved_home_stores_precede_initialization;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower a void structured body after assigning every value that can be read
    /// following a (possibly conditional) call to a virtual callee-saved home.
    pub(crate) fn try_callee_saved_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        self.try_callee_saved_structured_body_impl(function, false)
    }

    /// The same virtual-register path with one uninitialized automatic array
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
        if changed {
            self.try_callee_saved_structured_body_impl(&normalized, true)
        } else {
            self.try_callee_saved_structured_body_impl(function, true)
        }
    }

    fn try_callee_saved_structured_body_impl(
        &mut self,
        function: &Function,
        with_frame_array: bool,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        let frame_array = if with_frame_array {
            let mut arrays = function
                .locals
                .iter()
                .filter(|local| local.array_length.is_some());
            let Some(array) = arrays.next() else {
                return Ok(false);
            };
            if arrays.next().is_some()
                || array.is_static
                || array.initializer.is_some()
                || array.data_bytes.is_some()
                || !matches!(array.declared_type, Type::Char | Type::UnsignedChar)
                || !((function.return_type == Type::Void && function.return_expression.is_none())
                    || (matches!(function.return_type, Type::Int | Type::UnsignedInt)
                        && function.return_expression.is_some()))
            {
                return Ok(false);
            }
            Some(array)
        } else {
            None
        };
        let supported_plain_return = (function.return_type == Type::Void
            && function.return_expression.is_none())
            || (matches!(function.return_type, Type::Int | Type::UnsignedInt)
                && function.return_expression.is_some());
        if (!with_frame_array && !supported_plain_return)
            || !supports_statements(
                &function.statements,
                function,
                &self.global_array_sizes,
                with_frame_array,
            )
        {
            return Ok(false);
        }

        let address_taken = crate::frame::collect_address_taken(function);
        let frame_scalar_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| {
                local.array_length.is_none() && address_taken.contains(local.name.as_str())
            })
            .collect();
        if frame_scalar_locals.iter().any(|local| {
            local.is_static
                || local.initializer.is_some()
                || class_of(local.declared_type).ok() != Some(ValueClass::General)
                || local.declared_type.width() > 32
        }) {
            return Ok(false);
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
                    .map(|parameter| parameter.name.as_str()),
            )
            .collect();
        let survivors: std::collections::HashSet<&str> = candidates
            .into_iter()
            .filter(|name| {
                read_after_possible_call(&function.statements, name, false).read_after_call
                    || (function_makes_call(function)
                        && function
                            .return_expression
                            .as_ref()
                            .is_some_and(|expression| expression_reads_name(expression, name)))
            })
            .collect();
        let call_accumulators = call_accumulator_names(function);
        // Entry-initialized locals rank ahead of incoming parameters. Deferred
        // locals introduced by nested declarations or inline expansion rank
        // after them and may share a home when their lifetimes do not overlap.
        let saved_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| {
                local.array_length.is_none()
                    && survivors.contains(local.name.as_str())
                    && !call_accumulators.contains(local.name.as_str())
            })
            .collect();
        if saved_locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || class_of(local.declared_type).ok() != Some(ValueClass::General)
                || (local.initializer.is_none()
                    && !is_definitely_assigned_before_reads(&function.statements, &local.name))
        }) {
            return Ok(false);
        }
        let saved_parameters: Vec<_> = function
            .parameters
            .iter()
            .rev()
            .filter(|parameter| survivors.contains(parameter.name.as_str()))
            .collect();
        if saved_parameters.iter().any(|parameter| {
            self.locations
                .get(&parameter.name)
                .is_none_or(|location| location.class != ValueClass::General)
        }) {
            return Ok(false);
        }
        let Some(ephemeral_locals) = plan_ephemeral_locals(function, &survivors, &address_taken)
        else {
            return Ok(false);
        };
        let (eager_saved_locals, deferred_saved_locals): (Vec<_>, Vec<_>) = saved_locals
            .into_iter()
            .partition(|local| local.initializer.is_some());
        let Some(deferred_home_plan) = plan_deferred_saved_homes(function, &deferred_saved_locals)
        else {
            return Ok(false);
        };

        let local_region_bytes = if let Some(array) = frame_array {
            let length = array.array_length.expect("frame array was gated");
            let Some(bytes) = u16::from(array.declared_type.width() / 8)
                .checked_mul(length)
                .filter(|bytes| *bytes != 0 && *bytes <= u16::from(u8::MAX))
            else {
                return Ok(false);
            };
            i16::try_from(bytes)
                .map_err(|_| Diagnostic::error("structured local frame is too large"))?
        } else {
            0
        };
        let global_member_search_entry = function.statements.first().is_some_and(|statement| {
            super::super::global_struct_member_search::is_global_struct_member_search_loop(
                statement,
                &self.global_array_sizes,
            )
        });
        let rounded_byte_pointer = global_member_search_entry
            .then(|| adjacent_byte_pointer_round_up_name(function))
            .flatten();

        let count =
            eager_saved_locals.len() + saved_parameters.len() + deferred_home_plan.group_count;
        let first_saved = 32usize.saturating_sub(count);
        let dense_entry_prefix = with_frame_array
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
        let homes: Vec<u8> = (0..count)
            .map(|home_index| {
                if global_member_search_entry && home_index >= deferred_preference_base {
                    let group = home_index - deferred_preference_base;
                    let rank = global_group_order
                        .iter()
                        .position(|candidate| *candidate == group)
                        .unwrap_or(group);
                    self.fresh_virtual_general_preferring(31u8.saturating_sub(rank as u8))
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
        let mut plan = mwcc_vreg::FramePlan::with_local_region(homes.clone(), local_region_bytes);
        if let Some(array) = frame_array {
            let extra_scalar_words = function
                .locals
                .iter()
                .filter(|local| {
                    local.array_length.is_none()
                        && !deferred_saved_locals
                            .iter()
                            .any(|saved| saved.name == local.name)
                        && !eager_saved_locals
                            .iter()
                            .any(|saved| saved.name == local.name)
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
                        && body_uses_local(&function.statements, &local.name)
                })
                .count();
            let array_offset = match self.behavior.frame_convention {
                FrameConvention::Predecrement => 8,
                FrameConvention::LinkageFirst => {
                    let words = if global_member_search_entry {
                        extra_scalar_words
                    } else {
                        self.entry_parameter_words + extra_scalar_words
                    };
                    8 + i16::try_from(words * 4).map_err(|_| {
                        Diagnostic::error("structured legacy local table is too large")
                    })?
                }
            };
            if self.behavior.frame_convention == FrameConvention::LinkageFirst {
                let occupied = i32::from(array_offset)
                    + i32::from(local_region_bytes)
                    + i32::try_from(4 * count).unwrap_or(i32::MAX);
                plan.frame_size = i16::try_from((occupied + 15) / 16 * 16)
                    .map_err(|_| Diagnostic::error("structured legacy frame is too large"))?;
            }
            self.frame_slots.insert(
                array.name.clone(),
                FrameSlot {
                    offset: array_offset,
                    class: ValueClass::General,
                    size: local_region_bytes as u8,
                    value_type: array.declared_type,
                    parameter_register: None,
                    is_array: true,
                },
            );
            let mut scalar_offset = array_offset;
            for local in &frame_scalar_locals {
                scalar_offset -= 4;
                self.frame_slots.insert(
                    local.name.clone(),
                    FrameSlot {
                        offset: scalar_offset,
                        class: ValueClass::General,
                        size: 4,
                        value_type: local.declared_type,
                        parameter_register: None,
                        is_array: false,
                    },
                );
            }
            let pointee = match array.declared_type {
                Type::Char => Pointee::Char,
                Type::UnsignedChar => Pointee::UnsignedChar,
                _ => unreachable!("structured frame array type was gated"),
            };
            self.locations.insert(
                array.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: GENERAL_SCRATCH,
                    signed: false,
                    width: 32,
                    pointee: Some(pointee),
                    stride: None,
                },
            );
        }
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTable;
        let dense_frame = with_frame_array
            && eager_saved_locals.is_empty()
            && count <= 18
            && (count >= 5 || (global_member_search_entry && count >= 4));
        let dense_save_helper =
            dense_frame && self.behavior.frame_convention == FrameConvention::Predecrement;
        let dense_inline_save =
            dense_frame && self.behavior.frame_convention == FrameConvention::LinkageFirst;
        if dense_frame {
            self.output.pre_scheduled = true;
        }
        if dense_inline_save {
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
                    s: first_saved as u8,
                    a: 1,
                    offset: plan.frame_size - 4 * count as i16,
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
        if dense_save_helper {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 11,
                a: 1,
                immediate: plan.frame_size,
            });
            let helper = format!("_savegpr_{first_saved}");
            self.record_relocation(RelocationKind::Rel24, &helper);
            self.output
                .instructions
                .push(Instruction::BranchAndLink { target: helper });
        }

        let batched_saved_home_stores = saved_home_stores_precede_initialization(
            self.behavior.frame_convention,
            eager_saved_locals.len(),
            saved_parameters.len(),
            deferred_home_plan.group_count,
        );

        let saved_parameter_base = eager_saved_locals.len();
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
        if batched_saved_home_stores {
            self.emit_structured_saved_home_store_range(
                &homes[..saved_parameter_base],
                0,
                plan.frame_size,
            );
            for (parameter_index, (_, home, incoming)) in saved_parameter_homes.iter().enumerate() {
                self.emit_structured_saved_home_store(
                    *home,
                    saved_parameter_base + parameter_index,
                    plan.frame_size,
                );
                self.output
                    .instructions
                    .push(Instruction::move_register(*home, *incoming));
            }
            self.emit_structured_saved_home_store_range(
                &homes[deferred_home_base..],
                deferred_home_base,
                plan.frame_size,
            );
        }

        let mut home_index = 0;
        for local in eager_saved_locals {
            let home = homes[home_index];
            home_index += 1;
            if !batched_saved_home_stores && !dense_frame {
                self.emit_structured_saved_home_store(home, home_index - 1, plan.frame_size);
            }
            let initializer = local.initializer.as_ref().expect("partitioned as eager");
            if !self.try_emit_structured_wide_saved_initializer(initializer, home) {
                self.evaluate(initializer, local.declared_type, home)?;
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
        for (_, home, incoming) in &saved_parameter_homes {
            home_index += 1;
            if !batched_saved_home_stores {
                if !dense_frame {
                    self.emit_structured_saved_home_store(*home, home_index - 1, plan.frame_size);
                    self.output
                        .instructions
                        .push(Instruction::move_register(*home, *incoming));
                }
            }
        }
        debug_assert_eq!(home_index, deferred_home_base);
        for group in 0..deferred_home_plan.group_count {
            let slot_index = deferred_home_base + group;
            let home = homes[slot_index];
            if !batched_saved_home_stores && !dense_frame {
                self.emit_structured_saved_home_store(home, slot_index, plan.frame_size);
            }
        }
        for local in deferred_saved_locals {
            let group = deferred_home_plan.group(&local.name);
            let home = homes[deferred_home_base + group];
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
        self.try_preload_ephemeral_float_compare_literal(function, &ephemeral_locals)?;
        // Initializers are evaluated at declaration time, while an incoming
        // parameter still has its entry-register alias. MWCC can preserve that
        // alias after copying the value to a saved home (`mr r31,r3; lwz ...,r3`)
        // and switches subsequent body uses to the home only after declarations.
        for local in &ephemeral_locals {
            let class = class_of(local.declared_type).expect("eligibility checked");
            let alias = pure_local_alias(local)
                .and_then(|name| self.locations.get(name))
                .filter(|location| location.class == class)
                .map(|location| location.register);
            let temporary = alias.unwrap_or_else(|| match class {
                ValueClass::General if rounded_byte_pointer == Some(local.name.as_str()) => {
                    self.fresh_virtual_general_preferring(Eabi::general_result().number)
                }
                ValueClass::General => self.fresh_virtual_general(),
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
        self.plan_structured_float_handoff(function, &ephemeral_locals);
        let dense_statement_start = if dense_frame {
            if global_member_search_entry {
                0
            } else {
                self.emit_structured_dense_frame_entry(function, &saved_parameter_homes)?
                    .ok_or_else(|| {
                        Diagnostic::error(
                            "a dense structured frame needs a schedulable entry definition",
                        )
                    })?
            }
        } else {
            0
        };
        let alias_statements = if dense_frame {
            &function.statements[dense_statement_start..]
        } else {
            function.statements.as_slice()
        };
        let entry_parameter_alias = (!dense_inline_save)
            .then(|| plan_first_call_alias(alias_statements, &saved_parameter_homes))
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
        let statement_start = if dense_frame {
            dense_statement_start
        } else if entry_parameter_alias
            .as_ref()
            .is_some_and(|alias| alias.boundary == EntryAliasBoundary::AfterFirstStatement)
        {
            let alias = entry_parameter_alias.as_ref().expect("checked above");
            self.emit_structured_statements(
                &function.statements[..1],
                function,
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
                &function.statements[1..],
            );
            1
        } else {
            0
        };
        let mut condition_alias = entry_parameter_alias
            .filter(|alias| alias.boundary == EntryAliasBoundary::AfterFirstConditionTerm);
        self.emit_structured_statements(
            &function.statements[statement_start..],
            function,
            &ephemeral_locals,
            true,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut condition_alias,
        )?;
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
            self.normalize_structured_frame_argument_copies(first_saved as u8);
        }
        for (branch, label) in pending_gotos {
            let target = label_positions.get(&label).copied().ok_or_else(|| {
                Diagnostic::error(format!(
                    "structured forward branch targets an unknown label '{label}'"
                ))
            })?;
            if let Instruction::Branch {
                target: branch_target,
            } = &mut self.output.instructions[branch]
            {
                *branch_target = target;
            }
        }
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
        for branch in return_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        // Each source-level `if` creates a pair of optimizer labels even when
        // both collapse to direct instruction offsets. Build 163 exposes those
        // otherwise-hidden labels through the later unwind-symbol ordinal.
        self.output.anonymous_label_bump += structured_hidden_label_count(&function.statements);
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
        if dense_inline_save {
            self.output.instructions.extend([
                Instruction::LoadMultipleWord {
                    d: first_saved as u8,
                    a: 1,
                    offset: plan.frame_size - 4 * count as i16,
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
            let helper = format!("_restgpr_{first_saved}");
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
        self.schedule_legacy_inline_expansion_residue();
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
        let mut carried_condition_cache_restore = None;
        let mut scheduled_float_store = None;
        for (statement_index, statement) in statements.iter().enumerate() {
            let emitted_start = self.output.instructions.len();
            match statement {
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
                                self.begin_condition_global_cache(condition),
                                self.begin_condition_float_cache(condition),
                            )
                        };
                    let condition_result = (|| {
                        self.preload_condition_global_cache(condition)?;
                        let mut branches = Vec::with_capacity(terms.len());
                        for (term_index, term) in terms.iter().copied().enumerate() {
                            let retained_assertion_condition = if term_index == 0 {
                                None
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
                        Ok(branches)
                    })();
                    let carry_fallthrough_cache = matches!(
                        then_body.last(),
                        Some(Statement::Return(None) | Statement::Goto(_))
                    ) && matches!(
                        statements.get(statement_index + 1),
                        Some(Statement::If { else_body, .. }) if else_body.is_empty()
                    );
                    let continuation_cache = carry_fallthrough_cache.then(|| {
                        (
                            self.condition_global_values.clone(),
                            self.condition_float_cache.clone(),
                        )
                    });
                    self.restore_condition_global_cache(previous_cache);
                    self.restore_condition_float_cache(previous_float_cache);
                    let branches = condition_result?;
                    self.commit_structured_float_handoff();
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
                            " (inside structured if statement {statement_index})"
                        ));
                        diagnostic
                    })?;
                    let target = self.output.instructions.len();
                    for branch in branches {
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
                        .push(Instruction::Branch { target: 0 });
                }
                Statement::Return(None) => {
                    return_branches.push(self.output.instructions.len());
                    self.output
                        .instructions
                        .push(Instruction::Branch { target: 0 });
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
                    let terminal_volatile = matches!(declared_type, Type::Int | Type::UnsignedInt)
                        && value_read_before_redefinition(remaining, name)
                        && !read_after_possible_call(remaining, name, false).read_after_call;
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
                            (register >= mwcc_vreg::VIRTUAL_BASE)
                                .then(|| register - mwcc_vreg::VIRTUAL_BASE)
                        })
                        .and_then(|id| self.register_prefer.get(&u32::from(id)).copied());
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
                                    (location.register >= mwcc_vreg::VIRTUAL_BASE)
                                        .then(|| location.register - mwcc_vreg::VIRTUAL_BASE)
                                })
                                .and_then(|id| self.register_prefer.get(&u32::from(id)).copied())
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
                        let previous = previous.ok_or_else(|| {
                            Diagnostic::error("structured assignment has no register home")
                        })?;
                        let terminal_result = self.behavior.frame_convention
                            == FrameConvention::Predecrement
                            && statement_index + 1 == statements.len()
                            && in_place_call_combined_return_name(function) == Some(name.as_str());
                        let destination = if terminal_result {
                            Eabi::general_result().number
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
                                            let high = self.fresh_virtual_general();
                                            let scaled = self.fresh_virtual_general();
                                            self.emit_address_high(high, global);
                                            emit_scaled_index(
                                                &mut self.output.instructions,
                                                scaled,
                                                index_register,
                                                element_size,
                                            )?;
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
                                            self.output.instructions.push(Instruction::Add {
                                                d: destination,
                                                a: GENERAL_SCRATCH,
                                                b: scaled,
                                            });
                                            true
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
                            self.evaluate(value, declared_type, destination)
                        }
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (in structured assignment statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                        if terminal_result {
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
                _ => self.emit_statement(statement).map_err(|mut diagnostic| {
                    diagnostic.message.push_str(&format!(
                        " (in structured body statement {statement_index})"
                    ));
                    diagnostic
                })?,
            }
            self.schedule_dying_structured_local_argument(
                statement,
                &statements[statement_index + 1..],
                function,
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
        }
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

fn supports_statements(
    statements: &[Statement],
    function: &Function,
    global_array_sizes: &std::collections::HashMap<String, u32>,
    allow_global_search_loop: bool,
) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. }
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
        matches!(statement,
            Statement::Assign {
                name,
                value: Expression::Call { .. },
            } if name == candidate)
    })
}

fn structured_hidden_label_count(statements: &[Statement]) -> u32 {
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
                    + structured_hidden_label_count(then_body)
                    + structured_hidden_label_count(else_body)
            }
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
