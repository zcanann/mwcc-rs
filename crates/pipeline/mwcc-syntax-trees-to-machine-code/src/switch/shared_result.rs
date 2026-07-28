//! Whole-body switches whose case labels share one trailing return value.
//!
//! C commonly spells membership tests as a group of empty `case` arms followed
//! by `break`, a value-returning `default`, and one return after the switch.
//! Keeping this topology separate from general statement-switch lowering makes
//! the shared success block explicit and avoids cloning it once per case.

use super::Target;
use crate::generator::*;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{ArmBody, Function, Statement, Type};
use mwcc_target::Eabi;

impl Generator {
    pub(crate) fn try_shared_result_switch(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type == Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || crate::analysis::function_makes_call(function)
        {
            return Ok(false);
        }
        let Some(shared_result) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let [Statement::Switch {
            scrutinee,
            arms,
            default: Some(ArmBody::Return(default_result)),
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if arms.is_empty()
            || arms.iter().any(|arm| {
                !matches!(&arm.body, ArmBody::Statements(statements) if statements.is_empty())
            })
        {
            return Ok(false);
        }

        let register = match scrutinee {
            mwcc_syntax_trees::Expression::Variable(name) => {
                let Some(location) = self.locations.get(name) else {
                    return Ok(false);
                };
                if !matches!(location.class, ValueClass::General) {
                    return Ok(false);
                }
                location.register
            }
            _ => {
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            }
        };

        let mut values: Vec<i64> = arms.iter().map(|arm| arm.value).collect();
        values.sort_unstable();
        if values.windows(2).any(|pair| pair[0] == pair[1])
            || values
                .iter()
                .any(|&value| value < i16::MIN as i64 || value >= i16::MAX as i64)
        {
            return Ok(false);
        }
        let span = values[values.len() - 1] - values[0] + 1;
        if span > 6 && values.len() > 3 {
            return Ok(false);
        }

        let mut patches = Vec::new();
        self.lower_switch_range(
            register,
            &values,
            0,
            values.len() - 1,
            None,
            None,
            &mut patches,
        );

        // The default block is laid out first. A final unconditional branch to
        // it is therefore redundant: falling through reproduces mwcc's compact
        // membership-test topology.
        let dispatch_end = self.output.instructions.len();
        if dispatch_end != 0
            && patches.iter().any(|&(index, target)| {
                index == dispatch_end - 1 && matches!(target, Target::Default)
            })
            && matches!(
                self.output.instructions[dispatch_end - 1],
                Instruction::Branch { .. }
            )
        {
            self.output.instructions.pop();
            patches.retain(|&(index, _)| index != dispatch_end - 1);
        }

        let result = Eabi::general_result().number;
        let default_start = self.output.instructions.len();
        self.evaluate_tail(default_result, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let shared_start = self.output.instructions.len();
        self.evaluate_tail(shared_result, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        for (index, target) in patches {
            let destination = match target {
                Target::Body(_) => shared_start,
                Target::Default => default_start,
            };
            match &mut self.output.instructions[index] {
                Instruction::BranchConditionalForward { target, .. } => *target = destination,
                Instruction::Branch { target } => *target = destination,
                _ => unreachable!("switch patch points at a non-branch instruction"),
            }
        }
        Ok(true)
    }
}
