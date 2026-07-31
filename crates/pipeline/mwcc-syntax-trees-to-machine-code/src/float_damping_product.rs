//! Floating damping products of the form `x * (1 - scale * ABS(x))`.
//!
//! Legacy O0 MWCC retains the absolute-value diamond in a saved FPR, then
//! materializes the two constants and evaluates the remaining arithmetic
//! entirely through f0 before updating `x`.  The generic one-scratch walker
//! cannot represent the simultaneously live source, absolute value, and one.

use crate::analysis::structurally_equal;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_versions::Optimization;

struct FloatDampingProduct<'a> {
    value: &'a Expression,
    absolute: &'a Expression,
    scale: f64,
}

fn classify<'e>(
    left: &'e Expression,
    right: &'e Expression,
) -> Option<FloatDampingProduct<'e>> {
    let (value, damping) = if let Some(damping) = damping_factor(right) {
        (left, damping)
    } else {
        (right, damping_factor(left)?)
    };
    let absolute_value = crate::float_abs_select::abs_select_value(damping.0)?;
    structurally_equal(value, absolute_value).then_some(FloatDampingProduct {
        value,
        absolute: damping.0,
        scale: damping.1,
    })
}

fn damping_factor(expression: &Expression) -> Option<(&Expression, f64)> {
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::FloatLiteral(value) if *value == 1.0) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = right.as_ref()
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::FloatLiteral(scale), absolute)
        | (absolute, Expression::FloatLiteral(scale))
            if crate::float_abs_select::abs_select_value(absolute).is_some() =>
        {
            Some((absolute, *scale))
        }
        _ => None,
    }
}

impl Generator {
    pub(crate) fn try_emit_float_damping_product(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Multiply
            || double
            || self.behavior.optimization != Optimization::O0
        {
            return Ok(false);
        }
        let Some(damping) = classify(left, right) else {
            return Ok(false);
        };
        if !self.is_float_leaf(damping.value) {
            return Ok(false);
        }

        let source = self.float_register_of_leaf(damping.value)?;
        let absolute = self.fresh_virtual_float_preferring(27);
        if !self.try_emit_float_abs_select(damping.absolute, absolute)? {
            return Ok(false);
        }
        let one = self.fresh_virtual_float_preferring(1);
        self.load_float_literal(one, 1.0, false);
        self.load_float_literal(FLOAT_SCRATCH, damping.scale, false);
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: FLOAT_SCRATCH,
                a: FLOAT_SCRATCH,
                c: absolute,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: FLOAT_SCRATCH,
                a: one,
                b: FLOAT_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: destination,
                a: source,
                c: FLOAT_SCRATCH,
            });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::classify;
    use mwcc_syntax_trees::{BinaryOperator, ConditionalOrigin, Expression, UnaryOperator};

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn absolute(name: &str) -> Expression {
        let value = Expression::Variable(name.into());
        Expression::Conditional {
            condition: Box::new(binary(
                BinaryOperator::Less,
                value.clone(),
                Expression::IntegerLiteral(0),
            )),
            when_true: Box::new(Expression::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(value.clone()),
            }),
            when_false: Box::new(value),
            origin: ConditionalOrigin::Ternary,
        }
    }

    #[test]
    fn recognizes_a_self_damping_product_but_not_a_different_absolute_value() {
        let value = Expression::Variable("x".into());
        let scaled_absolute = binary(
            BinaryOperator::Multiply,
            Expression::FloatLiteral(0.015),
            absolute("x"),
        );
        let damping = binary(
            BinaryOperator::Subtract,
            Expression::FloatLiteral(1.0),
            scaled_absolute,
        );

        let shape = classify(&value, &damping).expect("self damping product");
        assert_eq!(shape.scale, 0.015);

        let other = binary(
            BinaryOperator::Subtract,
            Expression::FloatLiteral(1.0),
            binary(
                BinaryOperator::Multiply,
                Expression::FloatLiteral(0.015),
                absolute("y"),
            ),
        );
        assert!(classify(&value, &other).is_none());
    }
}
