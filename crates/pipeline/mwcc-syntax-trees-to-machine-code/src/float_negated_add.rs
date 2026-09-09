//! Negated floating arithmetic and its measured operand order.

use crate::generator::{Generator, FLOAT_SCRATCH};
use crate::operands::{float_combine, Operands};
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
    /// Register operands need no memory or call reordering. Normalize the
    /// negative sum/product identities MWCC uses, then reuse subtraction and
    /// contraction selection. A remaining direct negate stays on its original
    /// side of the operation, with a virtual lifetime protecting other inputs.
    pub(crate) fn try_emit_negated_float_arithmetic(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u32,
        double: bool,
    ) -> Compilation<bool> {
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply) {
            return Ok(false);
        }
        let negated = |expression: &Expression| {
            matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, .. })
        };
        let left_inner = match left {
            Expression::Unary { operator: UnaryOperator::Negate, operand } => operand.as_ref(),
            _ => left,
        };
        let right_inner = match right {
            Expression::Unary { operator: UnaryOperator::Negate, operand } => operand.as_ref(),
            _ => right,
        };
        let left_negative = negated(left);
        let right_negative = negated(right);
        if !left_negative && !right_negative {
            return Ok(false);
        }
        let optimize = self.behavior.simplify_negated_float_arithmetic;
        if operator == BinaryOperator::Add && left_negative && !right_negative
            && self.is_float_leaf(right)
            && matches!(left_inner, Expression::Binary { operator: BinaryOperator::Multiply, left: a, right: b }
                if self.is_float_leaf(a) && self.is_float_leaf(b))
        {
            if !optimize {
                let operands = self.place_float_operands(operator, left, right, destination, double)?;
                self.output.instructions.push(float_combine(operator, destination, operands, double)?);
                return Ok(true);
            }
            self.evaluate_float(&Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(right.clone()),
                right: Box::new(left_inner.clone()),
            }, destination)?;
            return Ok(true);
        }
        if !self.is_float_leaf(left_inner) || !self.is_float_leaf(right_inner) {
            return Ok(false);
        }
        if optimize && ((operator == BinaryOperator::Multiply && left_negative && right_negative)
            || (operator == BinaryOperator::Add && right_negative))
        {
            self.evaluate_float(&Expression::Binary {
                operator: if operator == BinaryOperator::Add { BinaryOperator::Subtract } else { operator },
                left: Box::new(if operator == BinaryOperator::Multiply { left_inner } else { left }.clone()),
                right: Box::new(right_inner.clone()),
            }, destination)?;
            return Ok(true);
        }
        let left_home = if left_negative {
            let preference = if !right_negative {
                FLOAT_SCRATCH
            } else if self.behavior.optimization == mwcc_versions::Optimization::O0 {
                3
            } else {
                self.float_register_of_leaf(left_inner)?
            };
            let home = self.fresh_virtual_float_preferring(preference);
            self.evaluate_float(left, home)?;
            home
        } else {
            self.float_register_of_leaf(left)?
        };
        let right_home = if right_negative {
            let home = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
            self.evaluate_float(right, home)?;
            home
        } else {
            self.float_register_of_leaf(right)?
        };
        self.output.instructions.push(float_combine(
            operator, destination, Operands::ordered(left_home, right_home)?, double,
        )?);
        Ok(true)
    }

    /// Lower `-leaf + call()` in MWCC's measured order. The call must happen
    /// before the negate so its `f1` result does not need another home; the
    /// negated leaf remains the first source of the commutative add.
    pub(crate) fn try_emit_negated_leaf_call_add(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u32,
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

        let call_result = u32::from(Eabi::float_result().number);
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
