//! Path-sensitive saved-home liveness for structured control flow.

use crate::analysis::*;
use mwcc_syntax_trees::Statement;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use mwcc_syntax_trees::Expression;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Flow {
    pub(super) read_after_call: bool,
    call_on_fallthrough: bool,
    falls_through: bool,
}

/// Whether `name` is read along a path after a call and therefore needs a
/// callee-saved home. Forward gotos retain the call state of every incoming
/// edge; returns and gotos terminate only their own fallthrough, allowing a
/// later label to resume analysis.
pub(super) fn read_after_possible_call(
    statements: &[Statement],
    name: &str,
    prior_call: bool,
) -> Flow {
    let mut pending_gotos = HashMap::<String, Vec<bool>>::new();
    let mut seen_labels = HashSet::<String>::new();
    flow(
        statements,
        name,
        prior_call,
        &mut pending_gotos,
        &mut seen_labels,
    )
}

fn flow(
    statements: &[Statement],
    name: &str,
    mut prior_call: bool,
    pending_gotos: &mut HashMap<String, Vec<bool>>,
    seen_labels: &mut HashSet<String>,
) -> Flow {
    let mut read_after = false;
    let mut falls_through = true;
    for statement in statements {
        if let Statement::Label(label) = statement {
            seen_labels.insert(label.clone());
            let incoming = pending_gotos.remove(label).unwrap_or_default();
            if falls_through || !incoming.is_empty() {
                prior_call = (falls_through && prior_call)
                    || incoming.into_iter().any(|call| call);
                falls_through = true;
            }
            continue;
        }
        if !falls_through {
            continue;
        }
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                read_after |= expression_reads_name_across_call(condition, name, prior_call);
                let branch_entry_call = prior_call || expression_has_call(condition);
                let then_flow = flow(
                    then_body,
                    name,
                    branch_entry_call,
                    pending_gotos,
                    seen_labels,
                );
                let else_flow = flow(
                    else_body,
                    name,
                    branch_entry_call,
                    pending_gotos,
                    seen_labels,
                );
                read_after |= then_flow.read_after_call || else_flow.read_after_call;
                let then_reaches = then_flow
                    .falls_through
                    .then_some(then_flow.call_on_fallthrough);
                let else_reaches = else_flow
                    .falls_through
                    .then_some(else_flow.call_on_fallthrough);
                match (then_reaches, else_reaches) {
                    (None, None) => falls_through = false,
                    (then_call, else_call) => {
                        prior_call = then_call.unwrap_or(false) || else_call.unwrap_or(false);
                    }
                }
            }
            Statement::Store { target, value } => {
                read_after |= expression_reads_name_across_call(target, name, prior_call)
                    || expression_reads_name_across_call(
                        value,
                        name,
                        prior_call || expression_has_call(target),
                    );
                prior_call |= statement_has_call(statement);
            }
            Statement::Assign {
                name: assigned_name,
                value,
            } => {
                read_after |= expression_reads_name_across_call(value, name, prior_call);
                if assigned_name == name {
                    prior_call = expression_has_call(value)
                        || (prior_call && expression_reads_name(value, name));
                } else {
                    prior_call |= statement_has_call(statement);
                }
            }
            Statement::Expression(value) => {
                read_after |= expression_reads_name_across_call(value, name, prior_call);
                prior_call |= statement_has_call(statement);
            }
            Statement::Return(expression) => {
                read_after |= expression.as_ref().is_some_and(|value| {
                    expression_reads_name_across_call(value, name, prior_call)
                });
                falls_through = false;
            }
            Statement::Goto(label) => {
                if seen_labels.contains(label) {
                    // A backward edge can revisit earlier reads after this call.
                    // Conservatively preserve the candidate rather than under-save.
                    read_after |= prior_call;
                } else {
                    pending_gotos
                        .entry(label.clone())
                        .or_default()
                        .push(prior_call);
                }
                falls_through = false;
            }
            Statement::Break | Statement::Continue => falls_through = false,
            Statement::Switch { .. } | Statement::Loop { .. } => {
                prior_call |= statement_has_call(statement);
            }
            Statement::Label(_) => unreachable!("labels are handled before reachability"),
        }
    }
    Flow {
        read_after_call: read_after,
        call_on_fallthrough: prior_call,
        falls_through,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments: vec![],
        })
    }

    #[test]
    fn conditional_calls_make_later_reads_survive() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![call("grow")],
                else_body: vec![],
            },
            Statement::Expression(Expression::Variable("pointer".into())),
        ];
        assert!(read_after_possible_call(&statements, "pointer", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "condition", false).read_after_call);
    }

    #[test]
    fn a_calling_arm_that_returns_does_not_reach_the_continuation() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![call("act"), Statement::Return(None)],
                else_body: vec![],
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_makes_reads_in_its_arm_live_across_the_call() {
        let statements = vec![Statement::If {
            condition: Expression::Call {
                name: "test".into(),
                arguments: vec![],
            },
            then_body: vec![Statement::Expression(Expression::Variable("value".into()))],
            else_body: vec![],
        }];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_fresh_assignment_kills_an_earlier_call_lifetime() {
        let statements = vec![
            call("before"),
            Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_forward_goto_carries_call_state_to_its_label() {
        let statements = vec![
            call("write"),
            Statement::Goto("error".into()),
            Statement::Return(None),
            Statement::Label("error".into()),
            Statement::Expression(Expression::Variable("card".into())),
        ];
        assert!(read_after_possible_call(&statements, "card", false).read_after_call);
    }

    #[test]
    fn a_pre_call_goto_does_not_invent_a_saved_lifetime() {
        let statements = vec![
            Statement::Goto("error".into()),
            call("unreachable"),
            Statement::Label("error".into()),
            Statement::Expression(Expression::Variable("card".into())),
        ];
        assert!(!read_after_possible_call(&statements, "card", false).read_after_call);
    }

    #[test]
    fn any_post_call_incoming_edge_requires_a_saved_home() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("failed_early".into()),
                then_body: vec![Statement::Goto("error".into())],
                else_body: vec![],
            },
            call("write"),
            Statement::Goto("error".into()),
            Statement::Label("error".into()),
            Statement::Expression(Expression::Variable("card".into())),
        ];
        assert!(read_after_possible_call(&statements, "card", false).read_after_call);
    }
}
