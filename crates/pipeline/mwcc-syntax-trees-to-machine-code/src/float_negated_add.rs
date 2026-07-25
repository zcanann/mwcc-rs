//! Operand placement for a negated float leaf added to a call result.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use mwcc_target::Eabi;

fn negated_operand_and_call<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, &'a Expression)> {
    if operator != BinaryOperator::Add {
        return None;
    }
    let negated = |expression: &'a Expression| match expression {
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => Some(operand.as_ref()),
        _ => None,
    };
    if let Some(operand) = negated(left) {
        Some((operand, right))
    } else {
        negated(right).map(|operand| (operand, left))
    }
}

impl Generator {
    /// Lower `-leaf + call()` in MWCC's measured order. The call must happen
    /// before the negate so its `f1` result does not need another home; the
    /// negated leaf remains the first source of the commutative add.
    pub(crate) fn try_emit_negated_leaf_call_add(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        let Some((operand, call)) = negated_operand_and_call(operator, left, right) else {
            return Ok(false);
        };
        if !matches!(operand, Expression::Variable(_))
            || !self.is_float_leaf(operand)
            || !self.is_float_call_value(call)
            || !self.float_location_survives_call(operand)
        {
            return Ok(false);
        }

        let call_result = Eabi::float_result().number;
        self.evaluate_float(call, call_result)?;
        let source = self.float_register_of_leaf(operand)?;
        let negated = self.fresh_virtual_float_preferring(source);
        self.output.instructions.push(Instruction::FloatNegate {
            d: negated,
            b: source,
        });
        self.output.instructions.push(if double {
            Instruction::FloatAddDouble {
                d: destination,
                a: negated,
                b: call_result,
            }
        } else {
            Instruction::FloatAddSingle {
                d: destination,
                a: negated,
                b: call_result,
            }
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::negated_operand_and_call;
    use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};

    #[test]
    fn recognizes_the_negated_side_on_either_side_of_an_add() {
        let negated = Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: Box::new(Expression::Variable("value".into())),
        };
        let call = Expression::Call {
            name: "sqrt".into(),
            arguments: Vec::new(),
        };

        assert!(negated_operand_and_call(BinaryOperator::Add, &negated, &call).is_some());
        assert!(negated_operand_and_call(BinaryOperator::Add, &call, &negated).is_some());
        assert!(
            negated_operand_and_call(BinaryOperator::Subtract, &negated, &call).is_none()
        );
    }
}
