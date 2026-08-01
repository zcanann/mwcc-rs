//! O0 projection placement inside a composed inline vector transaction.
//!
//! A three-component dot product owns a four-register descending expression
//! window even when it appears inside an assignment-valued comma chain.  Keep
//! that policy beside its semantic recognizer instead of teaching generic
//! floating assignment lowering about inline-expanded vector shapes.

use crate::analysis::expression_has_call;
use crate::float_materialized_condition::{
    is_three_component_squared_sum, same_scalar_expression,
};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type};

impl Generator {
    pub(crate) fn try_evaluate_materialized_float_projection(
        &mut self,
        value: &Expression,
        value_type: Type,
        destination: u8,
    ) -> Compilation<bool> {
        if !self.unoptimized_inline_float_transaction_homes || expression_has_call(value) {
            return Ok(false);
        }
        let window = if is_three_component_projection(value) {
            (4, 4)
        } else if is_negated_three_component_squared_sum(value) {
            (3, 3)
        } else {
            return Ok(false);
        };
        self.evaluate_materialized_float_assignment_value_in_window(
            value,
            value_type,
            destination,
            window,
        )?;
        Ok(true)
    }
}

fn is_negated_three_component_squared_sum(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Unary {
            operator: mwcc_syntax_trees::UnaryOperator::Negate,
            operand,
        } if is_three_component_squared_sum(operand)
    )
}

struct ProjectionTerm<'a> {
    offset: u32,
    factor_base: &'a Expression,
    minuend_base: &'a Expression,
    subtrahend_base: &'a Expression,
}

fn is_three_component_projection(expression: &Expression) -> bool {
    let mut expressions = Vec::new();
    collect_additive_terms(expression, &mut expressions);
    if expressions.len() != 3 {
        return false;
    }
    let Some(first) = projection_term(expressions[0]) else {
        return false;
    };
    let mut offsets = vec![first.offset];
    for expression in &expressions[1..] {
        let Some(term) = projection_term(expression) else {
            return false;
        };
        if !same_scalar_expression(first.factor_base, term.factor_base)
            || !same_scalar_expression(first.minuend_base, term.minuend_base)
            || !same_scalar_expression(first.subtrahend_base, term.subtrahend_base)
        {
            return false;
        }
        offsets.push(term.offset);
    }
    offsets.sort_unstable();
    offsets == [0, 4, 8]
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

fn projection_term(expression: &Expression) -> Option<ProjectionTerm<'_>> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    projection_term_parts(left, right).or_else(|| projection_term_parts(right, left))
}

fn projection_term_parts<'a>(
    factor: &'a Expression,
    difference: &'a Expression,
) -> Option<ProjectionTerm<'a>> {
    let (factor_base, offset) = float_member(factor)?;
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = difference
    else {
        return None;
    };
    let (minuend_base, minuend_offset) = float_member(left)?;
    let (subtrahend_base, subtrahend_offset) = float_member(right)?;
    (offset == minuend_offset && offset == subtrahend_offset).then_some(ProjectionTerm {
        offset,
        factor_base,
        minuend_base,
        subtrahend_base,
    })
}

fn float_member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float | Type::Pointer(Pointee::Float),
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((base, *offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }

    fn member(base: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(variable(base)),
            offset,
            member_type: Type::Float,
            index_stride: None,
        }
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn term(offset: u32) -> Expression {
        binary(
            BinaryOperator::Multiply,
            member("direction", offset),
            binary(
                BinaryOperator::Subtract,
                member("end", offset),
                member("start", offset),
            ),
        )
    }

    #[test]
    fn recognizes_a_three_component_projection() {
        let projection = binary(
            BinaryOperator::Add,
            term(8),
            binary(BinaryOperator::Add, term(0), term(4)),
        );
        assert!(is_three_component_projection(&projection));
    }

    #[test]
    fn rejects_crossed_components_and_inconsistent_vectors() {
        let crossed = binary(
            BinaryOperator::Multiply,
            member("direction", 4),
            binary(
                BinaryOperator::Subtract,
                member("end", 0),
                member("start", 0),
            ),
        );
        let inconsistent = binary(
            BinaryOperator::Add,
            term(8),
            binary(
                BinaryOperator::Add,
                term(0),
                binary(
                    BinaryOperator::Multiply,
                    member("other_direction", 4),
                    binary(
                        BinaryOperator::Subtract,
                        member("end", 4),
                        member("start", 4),
                    ),
                ),
            ),
        );
        assert!(projection_term(&crossed).is_none());
        assert!(!is_three_component_projection(&inconsistent));
    }

    #[test]
    fn recognizes_a_negated_three_component_norm() {
        let norm = Expression::Unary {
            operator: mwcc_syntax_trees::UnaryOperator::Negate,
            operand: Box::new(binary(
                BinaryOperator::Add,
                binary(BinaryOperator::Multiply, member("v", 8), member("v", 8)),
                binary(
                    BinaryOperator::Add,
                    binary(BinaryOperator::Multiply, member("v", 0), member("v", 0)),
                    binary(BinaryOperator::Multiply, member("v", 4), member("v", 4)),
                ),
            )),
        };
        assert!(is_negated_three_component_squared_sum(&norm));
    }
}
