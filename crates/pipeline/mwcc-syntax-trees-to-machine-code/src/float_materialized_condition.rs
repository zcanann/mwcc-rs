//! O0 comparison placement for a materialized three-component float norm.
//!
//! Composed inline vector transactions keep the zero literal above the entire
//! arithmetic window.  The generic comparison scheduler sees only one complex
//! operand and otherwise gives the literal f1, hiding the source-image lifetime
//! that makes MWCC choose f4.

use crate::analysis::expression_has_call;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type, UnaryOperator};

impl Generator {
    pub(crate) fn try_place_materialized_float_norm_literal_condition(
        &mut self,
        computed: &Expression,
        literal: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        if !self.unoptimized_inline_float_transaction_homes
            || !is_zero_literal(literal)
            || !is_three_component_squared_sum(computed)
            || expression_has_call(computed)
        {
            return Ok(None);
        }

        let literal_home = self.fresh_virtual_float_preferring(4);
        self.load_float_literal_into(literal_home, literal, double)?;
        self.evaluate_materialized_float_assignment_value(
            computed,
            if double { Type::Double } else { Type::Float },
            FLOAT_SCRATCH,
        )?;
        Ok(Some((FLOAT_SCRATCH, literal_home)))
    }
}

fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
        || matches!(expression, Expression::FloatLiteral(value) if *value == 0.0)
}

fn is_three_component_squared_sum(expression: &Expression) -> bool {
    let mut terms = Vec::new();
    collect_additive_terms(expression, &mut terms);
    terms.len() == 3 && terms.into_iter().all(is_squared_term)
}

fn collect_additive_terms<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    {
        collect_additive_terms(left, terms);
        collect_additive_terms(right, terms);
    } else {
        terms.push(expression);
    }
}

fn is_squared_term(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } if same_scalar_expression(left, right)
    )
}

fn same_scalar_expression(left: &Expression, right: &Expression) -> bool {
    match (left, right) {
        (Expression::Variable(left), Expression::Variable(right)) => left == right,
        (
            Expression::Member {
                base: left_base,
                offset: left_offset,
                member_type: left_type,
                index_stride: left_stride,
            },
            Expression::Member {
                base: right_base,
                offset: right_offset,
                member_type: right_type,
                index_stride: right_stride,
            },
        ) => {
            left_offset == right_offset
                && left_type == right_type
                && left_stride == right_stride
                && same_scalar_expression(left_base, right_base)
        }
        (
            Expression::Binary {
                operator: left_operator,
                left: left_a,
                right: left_b,
            },
            Expression::Binary {
                operator: right_operator,
                left: right_a,
                right: right_b,
            },
        ) => {
            left_operator == right_operator
                && same_scalar_expression(left_a, right_a)
                && same_scalar_expression(left_b, right_b)
        }
        (
            Expression::Unary {
                operator: left_operator,
                operand: left_operand,
            },
            Expression::Unary {
                operator: right_operator,
                operand: right_operand,
            },
        ) => {
            matches!(left_operator, UnaryOperator::Negate)
                && left_operator == right_operator
                && same_scalar_expression(left_operand, right_operand)
        }
        (
            Expression::Cast {
                target_type: left_type,
                operand: left_operand,
            },
            Expression::Cast {
                target_type: right_type,
                operand: right_operand,
            },
        ) => left_type == right_type && same_scalar_expression(left_operand, right_operand),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn square(name: &str) -> Expression {
        binary(
            BinaryOperator::Multiply,
            variable(name),
            variable(name),
        )
    }

    #[test]
    fn recognizes_three_squared_components_across_add_tree_shapes() {
        let norm = binary(
            BinaryOperator::Add,
            square("z"),
            binary(BinaryOperator::Add, square("x"), square("y")),
        );
        assert!(is_three_component_squared_sum(&norm));
    }

    #[test]
    fn rejects_a_mixed_product_and_the_wrong_component_count() {
        let mixed = binary(
            BinaryOperator::Add,
            square("z"),
            binary(
                BinaryOperator::Add,
                binary(BinaryOperator::Multiply, variable("x"), variable("y")),
                square("y"),
            ),
        );
        let pair = binary(BinaryOperator::Add, square("x"), square("y"));
        assert!(!is_three_component_squared_sum(&mixed));
        assert!(!is_three_component_squared_sum(&pair));
    }
}
