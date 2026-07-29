//! Float operand placement for a memory value combined with a conditional.
//!
//! A conditional arm may itself need the ordinary floating scratch register.
//! MWCC therefore completes the select in a second home before loading the
//! memory addend into `f0` and performing the enclosing add.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_target::Eabi;

fn located_add_and_conditional<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, &'a Expression)> {
    (operator == BinaryOperator::Add && matches!(right, Expression::Conditional { .. }))
        .then_some((left, right))
}

impl Generator {
    pub(crate) fn try_emit_float_memory_conditional_add(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        let Some((located, conditional)) = located_add_and_conditional(operator, left, right)
        else {
            return Ok(false);
        };
        if destination != FLOAT_SCRATCH || !self.is_float_located(located) {
            return Ok(false);
        }

        let selected = self.fresh_virtual_float_preferring(Eabi::float_result().number);
        self.evaluate_float(conditional, selected)?;
        self.emit_located_operand(located, destination)?;
        self.output.instructions.push(if double {
            Instruction::FloatAddDouble {
                d: destination,
                a: destination,
                b: selected,
            }
        } else {
            Instruction::FloatAddSingle {
                d: destination,
                a: destination,
                b: selected,
            }
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_a_conditional_right_addend() {
        let member = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 4,
            member_type: mwcc_syntax_trees::Type::Float,
            index_stride: None,
        };
        let conditional = Expression::Conditional {
            condition: Box::new(Expression::Variable("condition".into())),
            when_true: Box::new(Expression::FloatLiteral(1.0)),
            when_false: Box::new(Expression::FloatLiteral(2.0)),
            origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
        };

        assert!(located_add_and_conditional(BinaryOperator::Add, &member, &conditional).is_some());
        assert!(
            located_add_and_conditional(BinaryOperator::Subtract, &member, &conditional).is_none()
        );
    }
}
