//! Dense jump-table dispatch inside allocator-backed structured functions.
//!
//! Liveness consumes a canonical if-tree, while this emission owner receives
//! the source switch. That split is deliberate: source arms preserve shared
//! fallthrough bodies and source layout, both of which are lost when a switch
//! is cloned into mutually exclusive continuations.

use super::structured_entry_alias::EntryParameterAlias;
use super::structured_locals::body_uses_local;
use super::structured_switch_lowering::is_dense_structured_switch;
#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{JumpTable, RelocationTarget};
use mwcc_syntax_trees::ArmBody;
use mwcc_versions::JumpTableBaseStyle;

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_dense_switch(
        &mut self,
        scrutinee: &Expression,
        arms: &[mwcc_syntax_trees::SwitchArm],
        default: Option<&ArmBody>,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        if !is_dense_structured_switch(arms) {
            return Err(Diagnostic::error(
                "structured switch was retained without a dense dispatch plan",
            ));
        }

        let mut by_value = std::collections::HashMap::with_capacity(arms.len());
        let mut minimum = i64::MAX;
        let mut maximum = i64::MIN;
        for (source_index, arm) in arms.iter().enumerate() {
            if by_value.insert(arm.value, source_index).is_some() {
                return Err(Diagnostic::error("duplicate switch case values"));
            }
            minimum = minimum.min(arm.value);
            maximum = maximum.max(arm.value);
        }

        let subtract = minimum < 0 || minimum >= 3;
        let bound = if subtract { maximum - minimum } else { maximum };
        let negated_base = -minimum;
        if !(0..=u16::MAX as i64).contains(&bound)
            || (subtract && !(i16::MIN as i64..=i16::MAX as i64).contains(&negated_base))
        {
            return Err(Diagnostic::error(
                "structured jump-table index/base is out of immediate range",
            ));
        }

        let preserved_dispatch_values = self
            .locations
            .iter()
            .filter(|(name, location)| {
                location.class == ValueClass::General
                    && location.register == Eabi::general_result().number
                    && switch_bodies_use_name(arms, default, name)
            })
            .map(|(name, location)| (name.clone(), location.register))
            .collect::<Vec<_>>();

        let mut temporary_scrutinee = false;
        let scrutinee_register = match scrutinee {
            Expression::Variable(name) if self.locations.contains_key(name) => {
                let location = &self.locations[name];
                if location.class != ValueClass::General {
                    return Err(Diagnostic::error(
                        "structured switch scrutinee is not an integer",
                    ));
                }
                location.register
            }
            _ => {
                temporary_scrutinee = true;
                // PowerPC treats r0 as literal zero in `addi`'s base field.
                // A negative minimum rebases the value with `addi`, so first
                // evaluate a computed scrutinee in the next ABI scratch.
                let register = computed_scrutinee_register(subtract);
                self.evaluate_general(scrutinee, register)?;
                register
            }
        };
        for (offset, (name, source)) in preserved_dispatch_values.into_iter().enumerate() {
            let preferred = 7u8.saturating_sub(offset as u8);
            let retained = self.fresh_virtual_general_preferring(preferred);
            self.output
                .instructions
                .push(Instruction::move_register(retained, source));
            self.locations
                .get_mut(&name)
                .expect("dispatch value came from a known location")
                .register = retained;
        }

        let (index_register, table_register) = if subtract {
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: scrutinee_register,
                immediate: negated_base as i16,
            });
            // Rebasing moves the live index to r0, so the source local's home
            // is immediately reusable for the jump-table address. This is the
            // same lifetime rule as the ordinary switch owner, generalized
            // from its fixed r3 scrutinee to an allocator-backed local.
            let table_register = if temporary_scrutinee
                || scrutinee_register == GENERAL_SCRATCH
            {
                Eabi::general_result().number
            } else {
                scrutinee_register
            };
            (GENERAL_SCRATCH, table_register)
        } else {
            let table_register = if scrutinee_register == Eabi::general_result().number {
                4
            } else {
                Eabi::general_result().number
            };
            (scrutinee_register, table_register)
        };

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: index_register,
                immediate: bound as u16,
            });
        let out_of_range = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 0,
            });

        let table_index = self.output.jump_tables.len();
        let table_target = if table_index == 0 {
            RelocationTarget::JumpTable
        } else {
            RelocationTarget::JumpTableAt(table_index)
        };
        self.record_target(RelocationKind::Addr16Ha, table_target.clone());
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: table_register,
                a: 0,
                immediate: 0,
            });
        let load_base = if self.behavior.jump_table_base_style == JumpTableBaseStyle::EarlyInPlace {
            self.record_target(RelocationKind::Addr16Lo, table_target);
            self.output.instructions.push(Instruction::AddImmediate {
                d: table_register,
                a: table_register,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index_register,
                    shift: 2,
                });
            table_register
        } else {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index_register,
                    shift: 2,
                });
            self.record_target(RelocationKind::Addr16Lo, table_target);
            self.output.instructions.push(Instruction::AddImmediate {
                d: Eabi::general_result().number,
                a: table_register,
                immediate: 0,
            });
            Eabi::general_result().number
        };
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: GENERAL_SCRATCH,
            a: load_base,
            b: GENERAL_SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: GENERAL_SCRATCH });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);

        let mut body_offsets = vec![0u32; arms.len()];
        let mut join_branches = Vec::new();
        for (source_index, arm) in arms.iter().enumerate() {
            body_offsets[source_index] = self.output.instructions.len() as u32 * 4;
            self.reset_structured_switch_edge_caches();
            let falls_through_body = match &arm.body {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                    statements_fall_through(statements)
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target:
                            super::structured_early_return_schedule::STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                    false
                }
            };
            if falls_through_body
                && !arm.falls_through
                && (source_index + 1 != arms.len() || default.is_some())
            {
                join_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
            }
        }

        let default_offset = self.output.instructions.len() as u32 * 4;
        if let Some(default) = default {
            self.reset_structured_switch_edge_caches();
            match default {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target:
                            super::structured_early_return_schedule::STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                }
            }
        }
        let join = self.output.instructions.len();
        self.reset_structured_switch_edge_caches();

        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[out_of_range]
        {
            *target = default_offset as usize / 4;
        }
        for branch in join_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = join;
            }
        }

        let entries = (0..=bound)
            .map(|index| {
                let value = if subtract { minimum + index } else { index };
                by_value
                    .get(&value)
                    .map_or(default_offset, |source_index| body_offsets[*source_index])
            })
            .collect();
        let body_hidden_labels = arms
            .iter()
            .map(|arm| match &arm.body {
                ArmBody::Statements(statements) => {
                    super::structured::structured_hidden_label_count(statements)
                }
                ArmBody::Return(_) => 0,
            })
            .sum::<u32>()
            + default.map_or(0, |body| match body {
                ArmBody::Statements(statements) => {
                    super::structured::structured_hidden_label_count(statements)
                }
                ArmBody::Return(_) => 0,
            });
        let simple_terminal_calls = arms
            .iter()
            .map(|arm| &arm.body)
            .chain(default)
            .all(|body| {
                matches!(
                    body,
                    ArmBody::Statements(statements)
                        if matches!(
                            statements.as_slice(),
                            [Statement::Expression(
                                Expression::Call { .. } | Expression::CallThrough { .. }
                            )]
                                | [
                                    Statement::Expression(
                                        Expression::Call { .. } | Expression::CallThrough { .. }
                                    ),
                                    Statement::Return(None),
                                ]
                        )
                )
            });
        let labels_per_arm = if simple_terminal_calls {
            1
        } else {
            u32::from(
                self.behavior
                    .complex_structured_dense_switch_labels_per_arm,
            )
        };
        let base_labels = if simple_terminal_calls {
            1
        } else {
            u32::from(
                self.behavior
                    .complex_structured_dense_switch_base_labels,
            )
        };
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset: arms.len() as u32 * labels_per_arm
                + base_labels
                + u32::from(default.is_some())
                + body_hidden_labels,
        });
        // The table occupies its assigned `@N` slot. The writer's jump-table
        // walk lands on N rather than advancing past it, so retain the one
        // post-table ordinal needed by the next function-local static.
        self.output.post_constant_label_bump += 1;
        Ok(())
    }

    pub(super) fn reset_structured_switch_edge_caches(&mut self) {
        self.condition_global_values.clear();
        if let Some((name, register)) =
            self.structured_shared_switch_global_value.as_ref()
        {
            self.condition_global_values.insert(
                name.clone(),
                crate::condition_global_cache::ConditionGlobalValue::Register(
                    *register,
                ),
            );
        }
        self.condition_float_cache = Default::default();
        self.condition_member_cache = Default::default();
        self.wide_pair_mask_cache = Default::default();
        self.const_address_bases.clear();
        self.stored_globals.clear();
        self.transient_global_index_base = None;
        self.reuse_scratch_constant = false;
        self.scratch_constant = None;
        self.prematerialized_constants.clear();
        self.prematerialized_float_constants.clear();
    }
}

pub(super) fn switch_bodies_use_name(
    arms: &[mwcc_syntax_trees::SwitchArm],
    default: Option<&ArmBody>,
    name: &str,
) -> bool {
    arms.iter()
        .map(|arm| &arm.body)
        .chain(default)
        .any(|body| match body {
            ArmBody::Statements(statements) => body_uses_local(statements, name),
            ArmBody::Return(value) => crate::analysis::expression_reads_name(value, name),
        })
}

fn computed_scrutinee_register(requires_rebase: bool) -> u8 {
    if requires_rebase {
        Eabi::general_result().number + 1
    } else {
        GENERAL_SCRATCH
    }
}

pub(super) fn statements_fall_through(statements: &[Statement]) -> bool {
    let labels = statements
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| {
            let Statement::Label(name) = statement else {
                return None;
            };
            Some((name.as_str(), index))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut pending = vec![0usize];
    let mut visited = vec![false; statements.len() + 1];
    while let Some(index) = pending.pop() {
        if index == statements.len() {
            return true;
        }
        if visited[index] {
            continue;
        }
        visited[index] = true;
        let outcome = statement_control_outcome(&statements[index]);
        if outcome.falls_through {
            pending.push(index + 1);
        }
        for target in outcome.gotos {
            if let Some(&target) = labels.get(target.as_str()) {
                pending.push(target);
            }
        }
    }
    false
}

#[derive(Default)]
struct StatementControlOutcome {
    falls_through: bool,
    gotos: Vec<String>,
}

fn statement_control_outcome(statement: &Statement) -> StatementControlOutcome {
    match statement {
        Statement::Return(_) | Statement::Break | Statement::Continue => {
            StatementControlOutcome::default()
        }
        Statement::Goto(target) => StatementControlOutcome {
            falls_through: false,
            gotos: vec![target.clone()],
        },
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            let then_outcome = block_control_outcome(then_body);
            let else_outcome = block_control_outcome(else_body);
            StatementControlOutcome {
                falls_through:
                    then_outcome.falls_through || else_outcome.falls_through,
                gotos: then_outcome
                    .gotos
                    .into_iter()
                    .chain(else_outcome.gotos)
                    .collect(),
            }
        }
        _ => StatementControlOutcome {
            falls_through: true,
            gotos: Vec::new(),
        },
    }
}

fn block_control_outcome(statements: &[Statement]) -> StatementControlOutcome {
    let mut outcome = StatementControlOutcome {
        falls_through: true,
        gotos: Vec::new(),
    };
    for statement in statements {
        if !outcome.falls_through {
            break;
        }
        let statement_outcome = statement_control_outcome(statement);
        outcome.gotos.extend(statement_outcome.gotos);
        outcome.falls_through = statement_outcome.falls_through;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebased_computed_scrutinee_avoids_the_addi_zero_base() {
        assert_ne!(computed_scrutinee_register(true), GENERAL_SCRATCH);
        assert_eq!(
            computed_scrutinee_register(true),
            Eabi::general_result().number + 1
        );
        assert_eq!(computed_scrutinee_register(false), GENERAL_SCRATCH);
    }

    #[test]
    fn recognizes_two_terminal_if_arms_as_non_fallthrough() {
        let statements = [Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![Statement::Return(None)],
            else_body: vec![Statement::Goto("done".into())],
        }];
        assert!(!statements_fall_through(&statements));
    }

    #[test]
    fn retains_an_unlabeled_path_through_an_if() {
        let statements = [Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![Statement::Return(None)],
            else_body: Vec::new(),
        }];
        assert!(statements_fall_through(&statements));
    }

    #[test]
    fn an_internal_loop_goto_can_still_reach_the_arm_end() {
        let statements = [
            Statement::Goto("condition".into()),
            Statement::Label("body".into()),
            Statement::Label("condition".into()),
            Statement::If {
                condition: Expression::Variable("busy".into()),
                then_body: vec![Statement::Goto("body".into())],
                else_body: Vec::new(),
            },
            Statement::Assign {
                name: "done".into(),
                value: Expression::IntegerLiteral(1),
            },
        ];

        assert!(statements_fall_through(&statements));
    }

    #[test]
    fn a_goto_outside_the_arm_does_not_fall_through() {
        assert!(!statements_fall_through(&[Statement::Goto(
            "outside".into(),
        )]));
    }
}
