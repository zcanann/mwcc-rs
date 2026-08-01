//! One-edge reuse of an aggregate member loaded by an assignment and tested by
//! the immediately following guard.
//!
//! The condition-member cache normally begins at the guard. Legacy MWCC also
//! carries a full-width member value out of a side-effect-free assignment when
//! that exact member is read again by the next guard. The cache's instruction
//! liveness check remains the authority for rejecting intervening calls,
//! stores, or register definitions.

use crate::condition_member_cache::{cacheable_member, same_member};
use crate::generator::Generator;
use mwcc_syntax_trees::{Expression, Statement};

use super::structured_expression_visit::visit_expression;

pub(super) fn plan(
    generator: &Generator,
    statement: &Statement,
    next: Option<&Statement>,
) -> Option<Expression> {
    let Statement::Assign { value, .. } = statement else {
        return None;
    };
    let Some(Statement::If {
        condition,
        else_body,
        ..
    }) = next
    else {
        return None;
    };
    if !else_body.is_empty() || crate::analysis::expression_has_side_effect(value) {
        return None;
    }

    let mut assignment_members: Vec<Expression> = Vec::new();
    visit_expression(value, &mut |expression| {
        if cacheable_member(expression, generator) {
            assignment_members.push(expression.clone());
        }
    });
    if assignment_members.is_empty() {
        return None;
    }

    let mut reused = None;
    visit_expression(condition, &mut |expression| {
        if reused.is_none()
            && cacheable_member(expression, generator)
            && assignment_members
                .iter()
                .any(|member| same_member(member, expression))
        {
            reused = Some(expression.clone());
        }
    });
    reused
}
