//! Detect return computations that MWCC interleaves with a terminal store run.
//!
//! The list scheduler can fill a store-value materialization latency slot with
//! the first instruction of the return computation. Only stores in the
//! contiguous suffix immediately before the return participate: a call,
//! conditional, or any other statement is a scheduling barrier.

use super::*;

pub(super) fn has_terminal_store_return_hazard(
    statements: &[Statement],
    guards: &[GuardedReturn],
    return_expression: &Expression,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> bool {
    let terminal_stores: Vec<_> = statements
        .iter()
        .rev()
        .take_while(|statement| matches!(statement, Statement::Store { .. }))
        .collect();
    if terminal_stores.is_empty() {
        return false;
    }

    let target_is_pointer = |target: &Expression| match target {
        Expression::Dereference { .. } => true,
        Expression::Index { base, .. } | Expression::Member { base, .. } => {
            matches!(base.as_ref(), Expression::Variable(name)
                if !globals.contains_key(name.as_str())
                    && !global_array_sizes.contains_key(name.as_str()))
        }
        _ => false,
    };
    let value_needs_materialization = |value: &Expression| {
        !matches!(value, Expression::Variable(name) if !globals.contains_key(name.as_str()))
    };

    let has_pointer_store = terminal_stores.iter().any(|statement| {
        matches!(statement, Statement::Store { target, .. } if target_is_pointer(target))
    });
    let has_materialized_pointer_store = terminal_stores.iter().any(|statement| {
        matches!(statement, Statement::Store { target, value }
            if target_is_pointer(target) && value_needs_materialization(value))
    });

    let neg_leading_comparison = |condition: &Expression| {
        matches!(condition,
            Expression::Binary {
                operator: BinaryOperator::Greater | BinaryOperator::NotEqual,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(_)) && is_zero_literal(right))
    };
    let comparison_hoists = |condition: &Expression| -> bool {
        match condition {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } => matches!(operand.as_ref(), Expression::Variable(_)),
            Expression::Binary {
                operator,
                left,
                right,
            } if is_comparison(*operator) => {
                if !matches!(left.as_ref(), Expression::Variable(_)) {
                    return false;
                }
                if is_zero_literal(right) {
                    matches!(
                        operator,
                        BinaryOperator::Greater | BinaryOperator::NotEqual
                    )
                } else {
                    matches!(right.as_ref(), Expression::Variable(_))
                        || constant_value(right).is_some()
                }
            }
            _ => false,
        }
    };
    let single_const_guard_condition = if guards.len() == 1
        && constant_value(&guards[0].value).is_some()
        && constant_value(return_expression).is_some()
    {
        Some(&guards[0].condition)
    } else {
        None
    };
    let return_hoists_neg = neg_leading_comparison(return_expression)
        || single_const_guard_condition.is_some_and(neg_leading_comparison);
    let return_comparison_hoists = comparison_hoists(return_expression)
        || single_const_guard_condition.is_some_and(comparison_hoists);
    let return_is_computed_arithmetic = match return_expression {
        Expression::Binary { operator, .. } => {
            !is_comparison(*operator)
                && !matches!(
                    operator,
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                )
        }
        Expression::Unary { .. } => true,
        _ => false,
    };

    return_hoists_neg
        || (has_pointer_store && return_comparison_hoists)
        || (has_materialized_pointer_store && return_is_computed_arithmetic)
}

#[cfg(test)]
mod tests {
    use super::has_terminal_store_return_hazard;
    use mwcc_syntax_trees::{BinaryOperator, Expression, Statement};
    use std::collections::HashMap;

    fn pointer_store(value: Expression) -> Statement {
        Statement::Store {
            target: Expression::Dereference {
                pointer: Box::new(Expression::Variable("out".into())),
            },
            value,
        }
    }

    fn arithmetic_return() -> Expression {
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Variable("flags".into())),
            right: Box::new(Expression::IntegerLiteral(1)),
        }
    }

    #[test]
    fn materialized_terminal_pointer_store_has_a_return_schedule_hazard() {
        assert!(has_terminal_store_return_hazard(
            &[pointer_store(Expression::IntegerLiteral(3))],
            &[],
            &arithmetic_return(),
            &HashMap::new(),
            &HashMap::new(),
        ));
    }

    #[test]
    fn intervening_control_flow_ends_the_terminal_store_region() {
        let statements = [
            pointer_store(Expression::IntegerLiteral(3)),
            Statement::If {
                condition: Expression::Variable("restore".into()),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];
        assert!(!has_terminal_store_return_hazard(
            &statements,
            &[],
            &arithmetic_return(),
            &HashMap::new(),
            &HashMap::new(),
        ));
    }
}
