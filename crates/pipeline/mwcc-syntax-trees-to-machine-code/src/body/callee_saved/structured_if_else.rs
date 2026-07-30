//! Two-arm control flow for the structured virtual-register path.
//!
//! The parent owns liveness, frame construction, and the shared exit. This
//! module owns only the diamond: condition exits target the else arm and a
//! fallthrough then arm skips to the common continuation.

use super::structured::{logical_and_terms, logical_or_groups};
use super::structured_entry_alias::{fold_entry_alias_zero_test, EntryParameterAlias};
#[allow(unused_imports)]
use super::*;

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_if_else(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
        statement_index: usize,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        debug_assert!(!else_body.is_empty());
        if entry_alias.is_none()
            && self.try_emit_structured_if_else_cr_reuse(
                condition,
                then_body,
                else_body,
            )?
        {
            return Ok(());
        }
        let previous_wide_mask_cache = self.begin_wide_pair_mask_condition(condition);
        let previous_cache = self.begin_condition_global_cache(condition);
        let previous_float_cache = self.begin_composed_condition_float_cache(condition);
        let previous_member_cache = self.begin_condition_member_cache(condition);
        struct ConditionBranches {
            enter_then: Vec<usize>,
            enter_else: Vec<usize>,
        }
        let branches = (|| {
            self.preload_condition_global_cache(condition)?;
            if let Some(groups) = logical_or_groups(condition) {
                let mut enter_then = Vec::new();
                let mut enter_else = Vec::new();
                for (group_index, group) in groups.iter().enumerate() {
                    let last_group = group_index + 1 == groups.len();
                    let mut advance_group = Vec::new();
                    let mut next_group_float_cache = None;
                    for (term_index, term) in group.iter().copied().enumerate() {
                        let term_start = self.output.instructions.len();
                        let (options, condition_bit) =
                            self.emit_condition_test(term).map_err(|mut diagnostic| {
                                diagnostic.message.push_str(&format!(
                                    " (in structured if/else condition {statement_index})"
                                ));
                                diagnostic
                            })?;
                        self.reuse_short_circuit_member_base(term_index, term_start);
                        if statement_index == 0 && group_index == 0 && term_index == 0 {
                            if let Some(alias) = entry_alias.as_ref() {
                                fold_entry_alias_zero_test(&mut self.output.instructions, alias);
                            }
                        }
                        if !last_group && term_index == 0 {
                            next_group_float_cache = Some(self.condition_float_cache.clone());
                        }
                        let branch = self.output.instructions.len();
                        if !last_group && term_index + 1 == group.len() {
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options: options ^ 8,
                                    condition_bit,
                                    target: 0,
                                });
                            enter_then.push(branch);
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options,
                                    condition_bit,
                                    target: 0,
                                });
                            if last_group {
                                enter_else.push(branch);
                            } else {
                                advance_group.push(branch);
                            }
                        }
                        if statement_index == 0 && group_index == 0 && term_index == 0 {
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
                return Ok(ConditionBranches {
                    enter_then,
                    enter_else,
                });
            }
            let terms = logical_and_terms(condition);
            let mut enter_else = Vec::with_capacity(terms.len());
            for (term_index, term) in terms.into_iter().enumerate() {
                let (options, condition_bit) =
                    self.emit_condition_test(term).map_err(|mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (in structured if/else condition {statement_index})"
                        ));
                        diagnostic
                    })?;
                if statement_index == 0 && term_index == 0 {
                    if let Some(alias) = entry_alias.as_ref() {
                        fold_entry_alias_zero_test(&mut self.output.instructions, alias);
                    }
                }
                enter_else.push(self.output.instructions.len());
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
                enter_then: Vec::new(),
                enter_else,
            })
        })();
        self.restore_condition_member_cache(previous_member_cache);
        let retained_multiply_plan = condition_abs_value(condition).and_then(|value| {
            let source = self.observed_condition_float_register(value)?;
            let [first, second] = then_body else {
                return None;
            };
            Some((
                source,
                value.clone(),
                [
                    float_multiply_assignment(first, value)?,
                    float_multiply_assignment(second, value)?,
                ],
            ))
        });
        let else_wide_mask_cache = self.wide_pair_mask_false_edge_cache();
        self.wide_pair_mask_cache = Default::default();
        let then_float_cache = self.condition_float_literal_edge_cache();
        self.restore_condition_global_cache(previous_cache);
        self.restore_condition_float_cache(previous_float_cache);
        let branches = match branches {
            Ok(branches) => branches,
            Err(diagnostic) => {
                self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
                return Err(diagnostic);
            }
        };
        if let [branch] = branches.enter_else.as_slice() {
            self.schedule_frame_store_before_if_branch(*branch);
        }
        self.commit_structured_float_handoff();

        let then_start = self.output.instructions.len();
        for branch in branches.enter_then {
            self.patch_forward(branch, then_start);
        }
        let arm_previous_float_cache =
            std::mem::replace(&mut self.condition_float_cache, then_float_cache);
        let then_result = (|| {
            if let Some((source, value, assignments)) = retained_multiply_plan {
                let double = self.is_double_value(&value);
                for (destination_name, factor_name) in assignments {
                    let destination = self.float_register_of(&destination_name)?;
                    let factor = self.float_register_of(&factor_name)?;
                    self.output.instructions.push(if double {
                        Instruction::FloatMultiplyDouble {
                            d: destination,
                            a: source,
                            c: factor,
                        }
                    } else {
                        Instruction::FloatMultiplySingle {
                            d: destination,
                            a: source,
                            c: factor,
                        }
                    });
                }
            } else if !self.try_emit_structured_frame_bitfield_stores(then_body)? {
                self.emit_structured_arm_with_global_pointer_cache(
                    then_body,
                    function,
                    ephemeral_locals,
                    return_branches,
                    label_positions,
                    pending_gotos,
                    entry_alias,
                )
                .map_err(|mut diagnostic| {
                    diagnostic
                        .message
                        .push_str(&format!(" (inside structured then arm {statement_index})"));
                    diagnostic
                })?;
            }
            Ok(())
        })();
        self.restore_condition_float_cache(arm_previous_float_cache);
        if let Err(diagnostic) = then_result {
            self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
            return Err(diagnostic);
        }
        let skip_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let else_start = self.output.instructions.len();
        for branch in branches.enter_else {
            self.patch_forward(branch, else_start);
        }
        let retained_else_wide_mask_cache = else_body
            .first()
            .is_some_and(|statement| matches!(statement, Statement::If { .. }))
            .then_some(else_wide_mask_cache)
            .unwrap_or_default();
        let else_arm_previous_wide_mask_cache = std::mem::replace(
            &mut self.wide_pair_mask_cache,
            retained_else_wide_mask_cache,
        );
        let else_result = (|| {
            if !self.try_emit_shared_float_zero_assignments(else_body)?
                && !self.try_emit_structured_frame_bitfield_stores(else_body)?
            {
                self.emit_structured_arm_with_global_pointer_cache(
                    else_body,
                    function,
                    ephemeral_locals,
                    return_branches,
                    label_positions,
                    pending_gotos,
                    entry_alias,
                )
                .map_err(|mut diagnostic| {
                    diagnostic
                        .message
                        .push_str(&format!(" (inside structured else arm {statement_index})"));
                    diagnostic
                })?;
            }
            Ok(())
        })();
        self.restore_wide_pair_mask_cache(else_arm_previous_wide_mask_cache);
        if let Err(diagnostic) = else_result {
            self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
            return Err(diagnostic);
        }
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_else] {
            *target = join;
        }
        self.restore_wide_pair_mask_cache(previous_wide_mask_cache);
        Ok(())
    }

    /// Two float locals selected to zero in the same arm share one literal
    /// load. MWCC loads the first source-order destination, then copies it to
    /// the second; independently evaluating both assignments duplicates the
    /// pool relocation and loses the measured branch schedule.
    fn try_emit_shared_float_zero_assignments(
        &mut self,
        statements: &[Statement],
    ) -> Compilation<bool> {
        let [Statement::Assign {
            name: first,
            value: first_value,
        }, Statement::Assign {
            name: second,
            value: second_value,
        }] = statements
        else {
            return Ok(false);
        };
        if !crate::analysis::is_zero_literal(first_value)
            || !crate::analysis::is_zero_literal(second_value)
        {
            return Ok(false);
        }
        let (Ok(first_register), Ok(second_register)) = (
            self.float_register_of(first),
            self.float_register_of(second),
        ) else {
            return Ok(false);
        };
        let first_expression = Expression::Variable(first.clone());
        let second_expression = Expression::Variable(second.clone());
        let double = self.is_double_value(&first_expression);
        if first_register == second_register || double != self.is_double_value(&second_expression) {
            return Ok(false);
        }

        self.load_float_literal_into(first_register, first_value, double)?;
        self.output.instructions.push(Instruction::FloatMove {
            d: second_register,
            b: first_register,
        });
        Ok(true)
    }
}

fn condition_abs_value(condition: &Expression) -> Option<&Expression> {
    if let Some(value) = crate::float_abs_select::abs_select_value(condition) {
        return Some(value);
    }
    let Expression::Binary { left, right, .. } = condition else {
        return None;
    };
    crate::float_abs_select::abs_select_value(left)
        .or_else(|| crate::float_abs_select::abs_select_value(right))
}

fn float_multiply_assignment(
    statement: &Statement,
    shared: &Expression,
) -> Option<(String, String)> {
    let Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left,
                right,
            },
    } = statement
    else {
        return None;
    };
    let factor = if crate::analysis::structurally_equal(left, shared) {
        right.as_ref()
    } else if crate::analysis::structurally_equal(right, shared) {
        left.as_ref()
    } else {
        return None;
    };
    let Expression::Variable(factor) = factor else {
        return None;
    };
    Some((name.clone(), factor.clone()))
}
