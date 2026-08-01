//! Reuse of a full-width member load across adjacent scalar assignments.
//!
//! The first assignment may derive a value from a member while leaving the
//! member's load temporary live. If the immediately following assignment reads
//! the same member, optimized MWCC reuses that temporary instead of reloading
//! memory. The shared condition-member cache already proves instruction-level
//! liveness; this planner only establishes the two-statement semantic scope.

use crate::condition_member_cache::{cacheable_member, same_member};
use crate::generator::Generator;
use mwcc_syntax_trees::{Expression, Statement};

use super::structured_expression_visit::visit_expression;

pub(super) fn plan(
    generator: &Generator,
    statement: &Statement,
    next: Option<&Statement>,
) -> Option<Expression> {
    let Statement::Assign { value: first, .. } = statement else {
        return None;
    };
    let Some(Statement::Assign { value: second, .. }) = next else {
        return None;
    };
    if crate::analysis::expression_has_side_effect(first)
        || crate::analysis::expression_has_side_effect(second)
    {
        return None;
    }

    let mut first_members = Vec::new();
    visit_expression(first, &mut |expression| {
        if cacheable_member(expression, generator) {
            first_members.push(expression.clone());
        }
    });
    let mut reused = None;
    visit_expression(second, &mut |expression| {
        if reused.is_none()
            && cacheable_member(expression, generator)
            && first_members
                .iter()
                .any(|member| same_member(member, expression))
        {
            reused = Some(expression.clone());
        }
    });
    reused
}
