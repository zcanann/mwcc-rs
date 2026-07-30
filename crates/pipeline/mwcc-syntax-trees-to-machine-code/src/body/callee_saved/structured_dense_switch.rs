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
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
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
            (GENERAL_SCRATCH, Eabi::general_result().number)
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
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset: arms.len() as u32 + 1 + u32::from(default.is_some()),
        });
        // The table occupies its assigned `@N` slot. The writer's jump-table
        // walk lands on N rather than advancing past it, so retain the one
        // post-table ordinal needed by the next function-local static.
        self.output.post_constant_label_bump += 1;
        Ok(())
    }

    fn reset_structured_switch_edge_caches(&mut self) {
        self.condition_global_values.clear();
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

fn switch_bodies_use_name(
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

fn statements_fall_through(statements: &[Statement]) -> bool {
    let mut falls_through = true;
    for statement in statements {
        if !falls_through {
            break;
        }
        falls_through = match statement {
            Statement::Return(_) | Statement::Goto(_) | Statement::Break | Statement::Continue => {
                false
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } if !else_body.is_empty() => {
                statements_fall_through(then_body) || statements_fall_through(else_body)
            }
            _ => true,
        };
    }
    falls_through
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
