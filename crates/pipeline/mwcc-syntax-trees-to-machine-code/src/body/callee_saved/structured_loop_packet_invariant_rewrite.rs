//! Expression-tree mechanics for packet invariant extraction.
//!
//! The semantic proof belongs to `structured_loop_packet_invariants`; this
//! module only finds maximal accepted arithmetic subtrees and substitutes the
//! locals chosen for them.

#[allow(unused_imports)]
use super::*;

pub(super) fn collect_maximal<'a>(
    expression: &'a Expression,
    eligible: &impl Fn(&Expression) -> bool,
    output: &mut Vec<&'a Expression>,
) {
    if eligible(expression) {
        output.push(expression);
        return;
    }
    match expression {
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            collect_maximal(left, eligible, output);
            collect_maximal(right, eligible, output);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_maximal(operand, eligible, output);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_maximal(condition, eligible, output);
            collect_maximal(when_true, eligible, output);
            collect_maximal(when_false, eligible, output);
        }
        _ => {}
    }
}

pub(super) fn replace(
    expression: &Expression,
    replacements: &[(&Expression, String)],
) -> Expression {
    if let Some(name) = replacements.iter().find_map(|(candidate, name)| {
        crate::analysis::structurally_equal(candidate, expression).then_some(name)
    }) {
        return Expression::Variable(name.clone());
    }
    match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(replace(left, replacements)),
            right: Box::new(replace(right, replacements)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(replace(left, replacements)),
            right: Box::new(replace(right, replacements)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(replace(operand, replacements)),
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(replace(operand, replacements)),
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(replace(value, replacements)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(replace(condition, replacements)),
            when_true: Box::new(replace(when_true, replacements)),
            when_false: Box::new(replace(when_false, replacements)),
            origin: *origin,
        },
        _ => expression.clone(),
    }
}
