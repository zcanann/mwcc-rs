//! Algebraic cleanup for invariant display-list packet words.
//!
//! Macro expansion can separate balanced constants across adjacent arithmetic
//! nodes. MWCC reassociates those constants before instruction selection; this
//! module keeps that rewrite scoped to source-proven pure packet expressions.

#[allow(unused_imports)]
use super::*;

pub(super) fn simplify(expression: &Expression) -> Expression {
    let simplified = match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(simplify(left)),
            right: Box::new(simplify(right)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(simplify(operand)),
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(simplify(operand)),
        },
        _ => expression.clone(),
    };
    simplify_root(simplified)
}

fn simplify_root(expression: Expression) -> Expression {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return expression;
    };

    if operator == BinaryOperator::BitOr {
        let mut dynamic = Vec::new();
        let mut constant = 0u32;
        collect_or_terms(*left, &mut dynamic, &mut constant);
        collect_or_terms(*right, &mut dynamic, &mut constant);
        return rebuild_or(dynamic, constant);
    }

    if operator == BinaryOperator::Multiply {
        if crate::analysis::constant_value(&right) == Some(1) {
            return *left;
        }
        if crate::analysis::constant_value(&left) == Some(1) {
            return *right;
        }
    }

    if operator == BinaryOperator::Add {
        if let Some(constant) = crate::analysis::constant_value(&right) {
            if let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: minuend,
                right: subtrahend,
            } = left.as_ref()
            {
                if crate::analysis::constant_value(subtrahend) == Some(constant) {
                    return minuend.as_ref().clone();
                }
                if let Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left: base,
                    right: inner_constant,
                } = minuend.as_ref()
                {
                    if crate::analysis::constant_value(inner_constant) == Some(constant) {
                        return Expression::Binary {
                            operator: BinaryOperator::Subtract,
                            left: Box::new(base.as_ref().clone()),
                            right: Box::new(subtrahend.as_ref().clone()),
                        };
                    }
                }
            }
        }
    }

    Expression::Binary {
        operator,
        left,
        right,
    }
}

fn collect_or_terms(expression: Expression, dynamic: &mut Vec<Expression>, constant: &mut u32) {
    if let Some(value) = crate::analysis::constant_value(&expression) {
        *constant |= value as u32;
        return;
    }
    if let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = expression
    {
        collect_or_terms(*left, dynamic, constant);
        collect_or_terms(*right, dynamic, constant);
    } else {
        dynamic.push(expression);
    }
}

fn rebuild_or(mut dynamic: Vec<Expression>, constant: u32) -> Expression {
    if constant != 0 || dynamic.is_empty() {
        dynamic.push(Expression::IntegerLiteral(i64::from(constant)));
    }
    let mut terms = dynamic.into_iter();
    let first = terms.next().expect("an OR has at least one term");
    terms.fold(first, |left, right| Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: Box::new(left),
        right: Box::new(right),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn cancels_a_constant_across_an_intervening_subtraction() {
        let expression = binary(
            BinaryOperator::Add,
            binary(
                BinaryOperator::Subtract,
                binary(
                    BinaryOperator::Subtract,
                    Expression::Variable("a".into()),
                    Expression::IntegerLiteral(1),
                ),
                Expression::Variable("b".into()),
            ),
            Expression::IntegerLiteral(1),
        );
        let expected = binary(
            BinaryOperator::Subtract,
            Expression::Variable("a".into()),
            Expression::Variable("b".into()),
        );

        assert!(crate::analysis::structurally_equal(
            &simplify(&expression),
            &expected
        ));
    }

    #[test]
    fn preserves_unbalanced_constants() {
        let expression = binary(
            BinaryOperator::Add,
            binary(
                BinaryOperator::Subtract,
                Expression::Variable("a".into()),
                Expression::IntegerLiteral(2),
            ),
            Expression::IntegerLiteral(1),
        );

        assert!(crate::analysis::structurally_equal(
            &simplify(&expression),
            &expression
        ));
    }

    #[test]
    fn collects_packet_constants_after_the_dynamic_field() {
        let expression = binary(
            BinaryOperator::BitOr,
            binary(
                BinaryOperator::BitOr,
                Expression::IntegerLiteral(0xf500_0000),
                Expression::Variable("field".into()),
            ),
            Expression::IntegerLiteral(0x0088_0000),
        );
        let expected = binary(
            BinaryOperator::BitOr,
            Expression::Variable("field".into()),
            Expression::IntegerLiteral(0xf588_0000),
        );

        assert!(crate::analysis::structurally_equal(
            &simplify(&expression),
            &expected
        ));
    }
}
