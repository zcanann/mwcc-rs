//! Structured control flow whose register values survive conditional calls.
//!
//! This is the conservative bridge between semantic statement lowering and the
//! virtual-register allocator.  It owns a complete function only when every
//! statement is representable by the shared store/call emitter plus forward
//! `if` branches; unsupported control flow declines before emitting anything.

use super::structured_entry_alias::{
    fold_entry_alias_zero_test, plan_first_call_alias, EntryAliasBoundary, EntryParameterAlias,
};
use super::guarded_computed_survivor::emit_scaled_index;
use super::structured_frame_assignment::sink_single_use_parameter_assignment;
use super::structured_locals::{
    dead_ephemeral_float_locals, is_definitely_assigned_before_reads, plan_deferred_saved_homes,
    plan_ephemeral_locals,
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
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            if let Some(rewritten) = sink_single_use_parameter_assignment(function) {
                return self.try_callee_saved_structured_body_impl(&rewritten, true);
            }
        }
        self.try_callee_saved_structured_body_impl(function, true)
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
                || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
                || function.return_expression.is_none()
            {
                return Ok(false);
            }
            Some(array)
        } else {
            None
        };
        if (!with_frame_array
            && (function.return_type != Type::Void || function.return_expression.is_some()))
            || !supports_statements(&function.statements, function)
        {
            return Ok(false);
        }

        let candidates: Vec<&str> = function
            .locals
            .iter()
            .filter(|local| local.array_length.is_none())
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
                        && function.return_expression.as_ref().is_some_and(|expression| {
                            expression_reads_name(expression, name)
                        }))
            })
            .collect();
        // Entry-initialized locals rank ahead of incoming parameters. Deferred
        // locals introduced by nested declarations or inline expansion rank
        // after them and may share a home when their lifetimes do not overlap.
        let saved_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| {
                local.array_length.is_none() && survivors.contains(local.name.as_str())
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
        let Some(ephemeral_locals) = plan_ephemeral_locals(function, &survivors) else {
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

        let count =
            eager_saved_locals.len() + saved_parameters.len() + deferred_home_plan.group_count;
        let first_saved = 32usize.saturating_sub(count);
        let homes: Vec<u8> = (0..count)
            .map(|home_index| {
                if with_frame_array && eager_saved_locals.is_empty() && count <= 18 {
                    let preferred = if home_index < saved_parameters.len() {
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
                        && !eager_saved_locals.iter().any(|saved| saved.name == local.name)
                })
                .count();
            let array_offset = match self.behavior.frame_convention {
                FrameConvention::Predecrement => 8,
                FrameConvention::LinkageFirst => {
                    let words = self.entry_parameter_words + extra_scalar_words;
                    8 + i16::try_from(words * 4).map_err(|_| {
                        Diagnostic::error("structured legacy local table is too large")
                    })?
                }
            };
            if self.behavior.frame_convention == FrameConvention::LinkageFirst {
                let occupied = i32::from(array_offset)
                    + i32::from(local_region_bytes)
                    + i32::try_from(4 * count).unwrap_or(i32::MAX);
                plan.frame_size = i16::try_from((occupied + 15) / 16 * 16).map_err(|_| {
                    Diagnostic::error("structured legacy frame is too large")
                })?;
            }
            self.frame_slots.insert(
                array.name.clone(),
                FrameSlot {
                    offset: array_offset,
                    class: ValueClass::General,
                    size: local_region_bytes as u8,
                    parameter_register: None,
                    is_array: true,
                },
            );
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
            && count >= 5
            && count <= 18;
        let dense_save_helper =
            dense_frame && self.behavior.frame_convention == FrameConvention::Predecrement;
        let dense_inline_save = dense_frame
            && self.behavior.frame_convention == FrameConvention::LinkageFirst;
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
            self.evaluate(
                local.initializer.as_ref().expect("partitioned as eager"),
                local.declared_type,
                home,
            )?;
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
                    self.emit_structured_saved_home_store(
                        *home,
                        home_index - 1,
                        plan.frame_size,
                    );
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
            let temporary = match class {
                ValueClass::General => self.fresh_virtual_general(),
                ValueClass::Float => self.fresh_virtual_float_preferring(
                    self.ephemeral_float_home_preference(function, &ephemeral_locals),
                ),
            };
            if let Some(initializer) = &local.initializer {
                self.evaluate(initializer, local.declared_type, temporary)?;
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
        let dense_entry_emitted = if dense_frame {
            if !self.emit_structured_dense_frame_entry(function, &saved_parameter_homes)? {
                return Err(Diagnostic::error(
                    "a dense structured frame needs a schedulable entry definition",
                ));
            }
            true
        } else {
            false
        };
        let alias_statements = if dense_entry_emitted {
            &function.statements[1..]
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
        let statement_start = if dense_entry_emitted {
            1
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
            self.evaluate(return_expression, function.return_type, result)?;
        }
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

    fn emit_structured_statements(
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
                    let destination = self
                        .locations
                        .get(name)
                        .ok_or_else(|| {
                            Diagnostic::error("structured assignment has no register home")
                        })?
                        .register;
                    let handled_computed_address =
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
                                        self.record_relocation(RelocationKind::Addr16Lo, global);
                                        self.output.instructions.push(Instruction::AddImmediate {
                                            d: GENERAL_SCRATCH,
                                            a: high,
                                            immediate: 0,
                                        });
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
                        };
                    if handled_computed_address {
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

fn supports_statements(statements: &[Statement], function: &Function) -> bool {
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
        } => else_body.is_empty() && supports_statements(then_body, function),
        _ => false,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flow {
    read_after_call: bool,
    call_on_fallthrough: bool,
    falls_through: bool,
}

/// Path-sensitive call liveness for one structured statement sequence. A call
/// in an arm that returns does not contaminate the continuation, while a call in
/// the condition reaches either arm and can make their reads require a saved
/// home.
fn read_after_possible_call(statements: &[Statement], name: &str, mut prior_call: bool) -> Flow {
    let mut read_after = false;
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                read_after |= expression_reads_name_across_call(condition, name, prior_call);
                let branch_entry_call = prior_call || expression_has_call(condition);
                let then_flow = read_after_possible_call(then_body, name, branch_entry_call);
                let else_flow = read_after_possible_call(else_body, name, branch_entry_call);
                read_after |= then_flow.read_after_call || else_flow.read_after_call;
                let then_reaches = then_flow
                    .falls_through
                    .then_some(then_flow.call_on_fallthrough);
                let else_reaches = else_flow
                    .falls_through
                    .then_some(else_flow.call_on_fallthrough);
                match (then_reaches, else_reaches) {
                    (None, None) => {
                        return Flow {
                            read_after_call: read_after,
                            call_on_fallthrough: false,
                            falls_through: false,
                        };
                    }
                    (then_call, else_call) => {
                        prior_call = then_call.unwrap_or(false) || else_call.unwrap_or(false);
                    }
                }
            }
            Statement::Store { target, value } => {
                read_after |= expression_reads_name_across_call(target, name, prior_call)
                    || expression_reads_name_across_call(
                        value,
                        name,
                        prior_call || expression_has_call(target),
                    );
                prior_call |= statement_has_call(statement);
            }
            Statement::Assign {
                name: assigned_name,
                value,
            } => {
                read_after |= expression_reads_name_across_call(value, name, prior_call);
                if assigned_name == name {
                    // A fresh definition after a call kills the old value's
                    // cross-call lifetime. A self-referential update retains it.
                    prior_call = expression_has_call(value)
                        || (prior_call && expression_reads_name(value, name));
                } else {
                    prior_call |= statement_has_call(statement);
                }
            }
            Statement::Expression(value) => {
                read_after |= expression_reads_name_across_call(value, name, prior_call);
                prior_call |= statement_has_call(statement);
            }
            Statement::Return(expression) => {
                read_after |= expression.as_ref().is_some_and(|value| {
                    expression_reads_name_across_call(value, name, prior_call)
                });
                return Flow {
                    read_after_call: read_after,
                    call_on_fallthrough: false,
                    falls_through: false,
                };
            }
            Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_)
            | Statement::Switch { .. }
            | Statement::Loop { .. } => {
                prior_call |= statement_has_call(statement);
            }
        }
    }
    Flow {
        read_after_call: read_after,
        call_on_fallthrough: prior_call,
        falls_through: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_calls_make_later_reads_survive() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "grow".into(),
                    arguments: vec![],
                })],
                else_body: vec![],
            },
            Statement::Store {
                target: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("pointer".into())),
                },
                value: Expression::IntegerLiteral(1),
            },
        ];
        assert!(read_after_possible_call(&statements, "pointer", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "condition", false).read_after_call);
    }

    #[test]
    fn a_calling_arm_that_returns_does_not_reach_the_continuation() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![
                    Statement::Expression(Expression::Call {
                        name: "act".into(),
                        arguments: vec![],
                    }),
                    Statement::Return(None),
                ],
                else_body: vec![],
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_makes_reads_in_its_arm_live_across_the_call() {
        let statements = vec![Statement::If {
            condition: Expression::Call {
                name: "test".into(),
                arguments: vec![],
            },
            then_body: vec![Statement::Expression(Expression::Variable("value".into()))],
            else_body: vec![],
        }];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_fresh_assignment_kills_an_earlier_call_lifetime() {
        let statements = vec![
            Statement::Expression(Expression::Call {
                name: "before".into(),
                arguments: vec![],
            }),
            Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);

        let self_update = vec![
            Statement::Expression(Expression::Call {
                name: "before".into(),
                arguments: vec![],
            }),
            Statement::Assign {
                name: "value".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("value".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            },
        ];
        assert!(read_after_possible_call(&self_update, "value", false).read_after_call);
    }
}
