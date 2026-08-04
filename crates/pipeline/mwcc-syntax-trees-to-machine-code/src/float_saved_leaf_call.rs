//! Arithmetic between a call result and a call-surviving float leaf.
//!
//! A direct floating call already produces its value in f1. When the other
//! operand has a callee-saved home, MWCC consumes that f1 value directly rather
//! than copying it through the ordinary expression scratch (f0).

use crate::generator::Generator;
use crate::operands::{float_combine, Operands};
use mwcc_core::Compilation;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_target::Eabi;

fn leaf_and_call<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, &'a Expression, bool)> {
    if !matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
    ) {
        return None;
    }
    match (left, right) {
        (call @ (Expression::Call { .. } | Expression::VirtualCall { .. }), leaf @ Expression::Variable(_)) => {
            Some((leaf, call, true))
        }
        (leaf @ Expression::Variable(_), call @ (Expression::Call { .. } | Expression::VirtualCall { .. })) => {
            Some((leaf, call, false))
        }
        _ => None,
    }
}

impl Generator {
    /// Consume a direct floating call result in f1 when its leaf partner is
    /// already preserved across the call. Source order is retained for
    /// subtraction and division; the ordinary combiner handles commutative
    /// operand ordering for addition and multiplication.
    pub(crate) fn try_emit_saved_float_leaf_call_arithmetic(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        let Some((leaf, call, call_is_left)) = leaf_and_call(operator, left, right) else {
            return Ok(false);
        };
        if !self.is_float_leaf(leaf)
            || !self.is_float_call_value(call)
            || !self.float_location_survives_call(leaf)
        {
            return Ok(false);
        }

        let call_result = Eabi::float_result().number;
        self.evaluate_float(call, call_result)?;
        let leaf = self.float_register_of_leaf(leaf)?;
        let operands = if call_is_left {
            Operands::ordered(call_result, leaf)?
        } else {
            Operands::ordered(leaf, call_result)?
        };
        self.output
            .instructions
            .push(float_combine(operator, destination, operands, double)?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::leaf_and_call;
    use mwcc_syntax_trees::{BinaryOperator, Expression};

    fn call() -> Expression {
        Expression::Call {
            name: "sample".into(),
            arguments: Vec::new(),
        }
    }

    #[test]
    fn recognizes_both_source_orders_and_preserves_the_call_side() {
        let leaf = Expression::Variable("value".into());
        let call = call();
        let right_call = leaf_and_call(BinaryOperator::Multiply, &leaf, &call).unwrap();
        assert!(!right_call.2);

        let left_call = leaf_and_call(BinaryOperator::Subtract, &call, &leaf).unwrap();
        assert!(left_call.2);
    }

    #[test]
    fn rejects_non_arithmetic_and_non_leaf_pairs() {
        let leaf = Expression::Variable("value".into());
        let computed = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(leaf.clone()),
            right: Box::new(Expression::FloatLiteral(1.0)),
        };
        assert!(leaf_and_call(BinaryOperator::BitAnd, &leaf, &call()).is_none());
        assert!(leaf_and_call(BinaryOperator::Multiply, &computed, &call()).is_none());
    }
}
