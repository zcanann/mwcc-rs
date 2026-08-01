//! Guarded statement switches whose arms conditionally replace one local result.
//!
//! This is a single shared-result graph: the initializer occupies the return
//! register, the outer guard can skip the switch, and every case-local guard
//! either replaces that register or joins the common leaf return.

use super::Target;
use crate::analysis::constant_value;
use crate::generator::*;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};
use mwcc_target::Eabi;

struct ResultArm<'a> {
    value: i64,
    condition: &'a Expression,
    replacement: i64,
}

impl Generator {
    pub(crate) fn try_guarded_result_switch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type == Type::Void
            || function.locals.len() != 1
            || !function.guards.is_empty()
            || crate::analysis::function_makes_call(function)
        {
            return Ok(false);
        }
        let local = &function.locals[0];
        let Some(initial) = local.initializer.as_ref().and_then(constant_value) else {
            return Ok(false);
        };
        if !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &local.name
        ) {
            return Ok(false);
        }
        let [Statement::If {
            condition: outer_condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Switch {
            scrutinee,
            arms,
            default,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        let default_is_empty = match default {
            None => true,
            Some(ArmBody::Statements(statements)) => statements.is_empty(),
            Some(ArmBody::Return(_)) => false,
        };
        if !else_body.is_empty() || arms.is_empty() || !default_is_empty {
            return Ok(false);
        }

        let mut result_arms = Vec::with_capacity(arms.len());
        for arm in arms {
            let ArmBody::Statements(statements) = &arm.body else {
                return Ok(false);
            };
            let [Statement::If {
                condition,
                then_body,
                else_body,
            }] = statements.as_slice()
            else {
                return Ok(false);
            };
            let [Statement::Assign { name, value }] = then_body.as_slice() else {
                return Ok(false);
            };
            let Some(replacement) = constant_value(value) else {
                return Ok(false);
            };
            if arm.falls_through || !else_body.is_empty() || name != &local.name {
                return Ok(false);
            }
            result_arms.push(ResultArm {
                value: arm.value,
                condition,
                replacement,
            });
        }
        let mut sorted_values: Vec<i64> = result_arms.iter().map(|arm| arm.value).collect();
        sorted_values.sort_unstable();
        if sorted_values.windows(2).any(|pair| pair[0] == pair[1])
            || sorted_values
                .iter()
                .any(|value| *value < i16::MIN as i64 || *value >= i16::MAX as i64)
            || sorted_values[sorted_values.len() - 1] - sorted_values[0] + 1 > 6
        {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        let (outer_options, outer_bit) = self.emit_condition_test(outer_condition)?;
        let outer_skip = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: outer_options,
                condition_bit: outer_bit,
                target: 0,
            });

        let switch_register = match scrutinee {
            Expression::Variable(name) => self.general_register_of(name)?,
            _ => {
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            }
        };
        let mut dispatch_patches = Vec::new();
        self.lower_switch_range(
            switch_register,
            &sorted_values,
            0,
            sorted_values.len() - 1,
            None,
            None,
            &mut dispatch_patches,
        );

        let sorted_index_by_value: std::collections::HashMap<i64, usize> = sorted_values
            .iter()
            .enumerate()
            .map(|(index, value)| (*value, index))
            .collect();
        let mut body_start = vec![0usize; result_arms.len()];
        let mut conditional_joins = Vec::with_capacity(result_arms.len());
        let mut unconditional_joins = Vec::with_capacity(result_arms.len().saturating_sub(1));
        for (source_index, arm) in result_arms.iter().enumerate() {
            body_start[sorted_index_by_value[&arm.value]] = self.output.instructions.len();
            let (options, condition_bit) = self.emit_condition_test(arm.condition)?;
            conditional_joins.push(self.output.instructions.len());
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.load_integer_constant(result, arm.replacement);
            if source_index + 1 != result_arms.len() {
                unconditional_joins.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
            }
        }

        let replacement_return = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let default_return = self.output.instructions.len();
        self.load_integer_constant(result, initial);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.patch_forward(outer_skip, default_return);
        for branch in conditional_joins {
            self.patch_forward(branch, default_return);
        }
        for branch in unconditional_joins {
            let Instruction::Branch { target } = &mut self.output.instructions[branch] else {
                unreachable!()
            };
            *target = replacement_return;
        }
        for (branch, target) in dispatch_patches {
            let destination = match target {
                Target::Body(index) => body_start[index],
                Target::Default => default_return,
            };
            match &mut self.output.instructions[branch] {
                Instruction::BranchConditionalForward { target, .. } => *target = destination,
                Instruction::Branch { target } => *target = destination,
                _ => unreachable!("switch patch points at a non-branch instruction"),
            }
        }
        self.output.anonymous_label_bump += 4 * result_arms.len() as u32 + 4;
        Ok(true)
    }
}
