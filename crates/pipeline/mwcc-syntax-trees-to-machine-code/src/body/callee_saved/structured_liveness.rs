//! Path-sensitive saved-home liveness for structured control flow.

use crate::analysis::*;
use mwcc_syntax_trees::{Expression, Function, LoopKind, Statement};
use std::collections::{HashMap, HashSet};

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

/// Whether `name` needs a saved home across the structured body and its
/// fallthrough return expression. Reads used only to marshal a call's
/// arguments happen before that call and therefore remain volatile-safe.
pub(crate) fn read_after_possible_call_in_return(
    statements: &[Statement],
    return_expression: Option<&Expression>,
    name: &str,
) -> bool {
    let body = read_after_possible_call(statements, name, false);
    body.read_after_call
        || (body.falls_through
            && return_expression.is_some_and(|expression| {
                expression_reads_name_across_call(expression, name, body.call_on_fallthrough)
            }))
}

/// Whether `name` needs a saved home across declaration initializers, the
/// structured body, and its fallthrough return expression.
///
/// Initializers execute in source order before the body. A call in one
/// initializer therefore clobbers volatile parameters and earlier locals
/// before later initializers or statements can read them. Reaching the named
/// local's own declaration starts its new lifetime after its initializer has
/// completed, so calls from preceding declarations do not leak into it.
pub(crate) fn read_after_possible_call_in_function(
    function: &Function,
    name: &str,
) -> bool {
    let mut read_after_call = false;
    let mut prior_call = false;
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            read_after_call |=
                expression_reads_name_across_call(initializer, name, prior_call);
            if local.name != name {
                prior_call |= expression_has_call(initializer);
            }
        }
        if local.name == name {
            prior_call = false;
        }
    }

    let body = read_after_possible_call(&function.statements, name, prior_call);
    read_after_call
        || body.read_after_call
        || (body.falls_through
            && function.return_expression.as_ref().is_some_and(|expression| {
                expression_reads_name_across_call(
                    expression,
                    name,
                    body.call_on_fallthrough,
                )
            }))
}

/// Whether a local is defined by a terminal call, consumed by the immediately
/// following zero test, and dead on both successors.
///
/// Inline expansion can preserve a conservative source-survivor marker after
/// turning `result = wrapper(); if (!result) return ...;` into a comma-sequenced
/// call transaction. The result is born after those calls and dies in the
/// adjacent condition, so it belongs in the volatile ABI result register.
pub(super) fn is_immediate_call_result_zero_guard(function: &Function, name: &str) -> bool {
    immediate_call_result_zero_guard(
        &function.statements,
        function.return_expression.as_ref(),
        name,
    )
}

/// Whether inline expansion introduced a one-expression alias for the final
/// call result of a comma transaction.
///
/// The canonical form is `(temporary = call(...), temporary)`. Its only read
/// is sequenced immediately after its definition, so source-level survivor
/// provenance must not force it into a callee-saved home.
pub(super) fn is_inline_terminal_call_result_alias(function: &Function, name: &str) -> bool {
    inline_terminal_call_result_alias(
        &function.statements,
        function.return_expression.as_ref(),
        name,
    )
}

fn inline_terminal_call_result_alias(
    statements: &[Statement],
    return_expression: Option<&Expression>,
    name: &str,
) -> bool {
    let mut owner = None;
    for (index, statement) in statements.iter().enumerate() {
        let expression = match statement {
            Statement::Assign { value, .. } | Statement::Expression(value) => value,
            _ => continue,
        };
        if crate::analysis::count_name_occurrences(expression, name) == 2
            && contains_terminal_call_result_alias(expression, name)
        {
            if owner.replace(index).is_some() {
                return false;
            }
        }
    }
    let Some(owner) = owner else {
        return false;
    };
    !statements
        .iter()
        .enumerate()
        .any(|(index, statement)| index != owner && statement_reads_name(statement, name))
        && !return_expression
            .is_some_and(|expression| expression_reads_name(expression, name))
}

fn contains_terminal_call_result_alias(
    expression: &Expression,
    name: &str,
) -> bool {
    let Expression::Comma { left, right } = expression else {
        return false;
    };
    let packet = matches!(left.as_ref(), Expression::Assign { target, value }
        if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name)
            && matches!(value.as_ref(), Expression::Call { .. })
            && !expression_reads_name(value, name))
        && terminal_alias_projection(right, name);
    packet
        || contains_terminal_call_result_alias(left, name)
        || contains_terminal_call_result_alias(right, name)
}

fn terminal_alias_projection(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Variable(read) => read == name,
        Expression::Comma { left, right } => {
            !expression_reads_name(left, name)
                && !expression_has_side_effect(left)
                && terminal_alias_projection(right, name)
        }
        _ => false,
    }
}

fn immediate_call_result_zero_guard(
    statements: &[Statement],
    return_expression: Option<&Expression>,
    name: &str,
) -> bool {
    let mut matched = None;
    for (index, window) in statements.windows(2).enumerate() {
        let [
            Statement::Assign {
                name: assigned,
                value,
            },
            Statement::If {
                condition,
                then_body,
                else_body,
            },
        ] = window
        else {
            continue;
        };
        if assigned == name
            && expression_ends_in_call(value)
            && is_zero_test_of(condition, name)
            && !then_body
                .iter()
                .chain(else_body)
                .any(|statement| statement_reads_name(statement, name))
        {
            if matched.replace(index).is_some() {
                return false;
            }
        }
    }
    let Some(assignment) = matched else {
        return false;
    };
    !statements[..assignment]
        .iter()
        .chain(&statements[assignment + 2..])
        .any(|statement| statement_reads_name(statement, name))
        && !return_expression
            .is_some_and(|expression| expression_reads_name(expression, name))
}

fn expression_ends_in_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. } => true,
        Expression::Comma { left, right } => {
            expression_ends_in_call(right)
                || matches!(left.as_ref(), Expression::Assign { target, value }
                    if matches!(target.as_ref(), Expression::Variable(assigned)
                        if terminal_alias_projection(right, assigned))
                        && matches!(value.as_ref(), Expression::Call { .. }))
        }
        _ => false,
    }
}

fn is_zero_test_of(expression: &Expression, name: &str) -> bool {
    let Expression::Binary {
        operator:
            mwcc_syntax_trees::BinaryOperator::Equal
            | mwcc_syntax_trees::BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return false;
    };
    (matches!(left.as_ref(), Expression::Variable(variable) if variable == name)
        && matches!(right.as_ref(), Expression::IntegerLiteral(0)))
        || (matches!(right.as_ref(), Expression::Variable(variable) if variable == name)
            && matches!(left.as_ref(), Expression::IntegerLiteral(0)))
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
    for (statement_index, statement) in statements.iter().enumerate() {
        let read_before_statement = read_after;
        let call_before_statement = prior_call;
        if let Statement::Label(label) = statement {
            seen_labels.insert(label.clone());
            let incoming = pending_gotos.remove(label).unwrap_or_default();
            if falls_through || !incoming.is_empty() {
                prior_call = (falls_through && prior_call) || incoming.into_iter().any(|call| call);
                falls_through = true;
            }
            continue;
        }
        if !falls_through {
            continue;
        }
        match statement {
            Statement::InlineAsm(_) => {
                prior_call |= statement_has_call(statement);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let fresh_condition_call_result =
                    condition_defines_fresh_call_result(condition, name);
                let fresh_condition_value = fresh_condition_call_result
                    || condition_defines_fresh_call_free_value(condition, name);
                if !fresh_condition_value {
                    read_after |=
                        expression_reads_name_across_call(condition, name, prior_call);
                }
                let branch_entry_call = if fresh_condition_value {
                    // The assignment defines a new value while evaluating the
                    // condition. Uses in either selected arm consume that
                    // fresh result; the candidate's earlier lifetime does not
                    // cross a call that preceded the condition.
                    false
                } else {
                    prior_call || expression_has_call(condition)
                };
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
                // The lvalue address is formed before its RHS and consumed by
                // the final store. A call in the RHS therefore makes every
                // value used to form that address call-spanning, even though
                // the source tree contains only the one target occurrence.
                read_after |= (expression_reads_name(target, name)
                    && expression_has_call(value))
                    || expression_reads_name_across_call(target, name, prior_call)
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
                read_after |= advance_expression(value, name, &mut prior_call);
                if assigned_name == name {
                    // The assignment's new value is defined only after every
                    // call in its right-hand side has returned. It has not yet
                    // crossed a call; a later statement must introduce one.
                    prior_call = false;
                } else {
                    prior_call |= statement_has_call(statement);
                }
            }
            Statement::Expression(value) => {
                read_after |= advance_expression(value, name, &mut prior_call);
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
                    // Preserve the candidate only when the revisited region
                    // actually reads it.  Lowered loops otherwise make every
                    // already-consumed local appear live across their calls.
                    let revisited_reads = statements[..statement_index]
                        .iter()
                        .rposition(
                            |statement| matches!(statement, Statement::Label(name) if name == label),
                        )
                        .map(|label_index| {
                            statements[label_index + 1..statement_index]
                                .iter()
                                .any(|statement| statement_reads_name(statement, name))
                        })
                        // A label owned by an enclosing block cannot be
                        // inspected from this recursive slice; retain the old
                        // conservative answer in that case.
                        .unwrap_or(true);
                    read_after |= prior_call && revisited_reads;
                } else {
                    pending_gotos
                        .entry(label.clone())
                        .or_default()
                        .push(prior_call);
                }
                falls_through = false;
            }
            Statement::Break | Statement::Continue => falls_through = false,
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => {
                if let Some(initializer) = initializer {
                    read_after |= advance_expression(initializer, name, &mut prior_call);
                }
                let mut iteration_call = prior_call;
                if *kind != LoopKind::DoWhile {
                    if let Some(condition) = condition {
                        read_after |=
                            advance_expression(condition, name, &mut iteration_call);
                    }
                }
                let body_flow = flow(
                    body,
                    name,
                    iteration_call,
                    pending_gotos,
                    seen_labels,
                );
                read_after |= body_flow.read_after_call;
                if body_flow.falls_through {
                    iteration_call = body_flow.call_on_fallthrough;
                    if let Some(step) = step {
                        read_after |= advance_expression(step, name, &mut iteration_call);
                    }
                    if let Some(condition) = condition {
                        // A do/while reaches its condition in this iteration;
                        // while/for reaches it on the backedge into the next.
                        read_after |=
                            advance_expression(condition, name, &mut iteration_call);
                    }
                    // The first iteration can introduce a call whose clobber
                    // reaches reads at the beginning of the next iteration.
                    // Call-state is a single bit, so replaying the body once
                    // from the backedge is sufficient to reach its fixed
                    // point. Definitions in the body still kill the incoming
                    // lifetime through the ordinary flow rules.
                    let mut backedge_gotos = HashMap::new();
                    let mut backedge_labels = HashSet::new();
                    read_after |= flow(
                        body,
                        name,
                        iteration_call,
                        &mut backedge_gotos,
                        &mut backedge_labels,
                    )
                    .read_after_call;
                }
                // A fallthrough do/while has executed its body at least once,
                // so the state after its condition is the actual exit state.
                // In particular, a final call-result assignment in the body
                // defines a fresh value; the mere presence of that call must
                // not make the value look clobbered at the following return.
                // Other loop forms may skip their bodies, and non-fallthrough
                // bodies may have break exits that are not represented by the
                // single fallthrough state, so retain the conservative join.
                if *kind == LoopKind::DoWhile && body_flow.falls_through {
                    prior_call = iteration_call;
                } else {
                    prior_call |= statement_has_call(statement);
                }
            }
            Statement::Switch { .. } => {
                prior_call |= statement_has_call(statement);
            }
            Statement::Label(_) => unreachable!("labels are handled before reachability"),
        }
        if !read_before_statement
            && read_after
            && std::env::var_os("MWCC_DIAGNOSTIC_LIVENESS")
                .is_some_and(|requested| requested == std::ffi::OsStr::new(name))
        {
            eprintln!(
                "structured liveness read for {name} at block statement {statement_index}, prior_call={call_before_statement}: {statement:?}"
            );
        }
    }
    Flow {
        read_after_call: read_after,
        call_on_fallthrough: prior_call,
        falls_through,
    }
}

pub(super) fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    fn arm_reads_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
        match body {
            mwcc_syntax_trees::ArmBody::Return(expression) => {
                expression_reads_name(expression, name)
            }
            mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                .iter()
                .any(|statement| statement_reads_name(statement, name)),
        }
    }

    match statement {
        // Embedded assembly can consume compiler locals without an ordinary
        // expression node. Keep backward-edge liveness conservative around it.
        Statement::InlineAsm(_) => true,
        Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Return(None) => false,
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => expression_reads_name(value, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body
                    .iter()
                    .chain(else_body)
                    .any(|statement| statement_reads_name(statement, name))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms.iter().any(|arm| arm_reads_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_reads_name(body, name))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .iter()
                .chain(condition)
                .chain(step)
                .any(|expression| expression_reads_name(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
        }
    }
}

/// Whether a condition defines `name` from one final direct-call result.
///
/// The assignment expression itself yields the result, so the comparison does
/// not reread the named local. With no sibling call or read, either selected arm
/// can continue using the volatile result register without a callee-saved home.
fn condition_defines_fresh_call_result(condition: &Expression, name: &str) -> bool {
    condition_fresh_call_result_callee(condition, name).is_some()
}

/// Whether both operands of a comparison are call-free values and one operand
/// freshly assigns `name`.
///
/// The assignment result feeds the comparison directly, so the old lifetime is
/// killed even when an unrelated call occurred in an earlier statement. Both
/// comparison operands are evaluated, unlike a short-circuit logical operator.
fn condition_defines_fresh_call_free_value(condition: &Expression, name: &str) -> bool {
    fn assignment_side(assigned: &Expression, sibling: &Expression, name: &str) -> bool {
        let Expression::Assign { target, value } = assigned else {
            return false;
        };
        matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name)
            && !expression_reads_name(value, name)
            && !expression_has_call(value)
            && !expression_reads_name(sibling, name)
            && !expression_has_call(sibling)
    }

    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        return false;
    };
    crate::analysis::is_comparison(*operator)
        && (assignment_side(left, right, name) || assignment_side(right, left, name))
}

fn condition_fresh_call_result_callee<'a>(
    condition: &'a Expression,
    name: &str,
) -> Option<&'a str> {
    fn assignment_side<'a>(
        assigned: &'a Expression,
        sibling: Option<&Expression>,
        name: &str,
    ) -> Option<&'a str> {
        let Expression::Assign { target, value } = assigned else {
            return None;
        };
        let Expression::Call { name: callee, .. } = value.as_ref() else {
            return None;
        };
        (matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name)
            && !expression_reads_name(value, name)
            && sibling.is_none_or(|sibling| {
                !expression_has_call(sibling)
                    && !expression_reads_name(sibling, name)
            }))
        .then_some(callee.as_str())
    }

    match condition {
        Expression::Assign { .. } => assignment_side(condition, None, name),
        Expression::Binary {
            operator:
                mwcc_syntax_trees::BinaryOperator::Equal
                | mwcc_syntax_trees::BinaryOperator::NotEqual
                | mwcc_syntax_trees::BinaryOperator::Less
                | mwcc_syntax_trees::BinaryOperator::LessEqual
                | mwcc_syntax_trees::BinaryOperator::Greater
                | mwcc_syntax_trees::BinaryOperator::GreaterEqual,
            left,
            right,
        } => {
            assignment_side(left, Some(right), name)
                .or_else(|| assignment_side(right, Some(left), name))
        }
        _ => None,
    }
}

pub(super) fn transient_condition_call_result_callee<'a>(
    statements: &'a [Statement],
    name: &str,
) -> Option<&'a str> {
    for statement in statements {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = statement
        else {
            continue;
        };
        if let Some(callee) = condition_fresh_call_result_callee(condition, name) {
            return Some(callee);
        }
        if let Some(callee) = transient_condition_call_result_callee(then_body, name)
            .or_else(|| transient_condition_call_result_callee(else_body, name))
        {
            return Some(callee);
        }
    }
    None
}

/// Advance call-state through one expression, treating a direct assignment as
/// a new definition after its right-hand side has been evaluated. This keeps a
/// loop counter assigned after a call from inheriting the old value's lifetime,
/// while still observing a read-modify-write step such as `i = i + 1`.
fn advance_expression(expression: &Expression, name: &str, prior_call: &mut bool) -> bool {
    match expression {
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name) =>
        {
            let read_after = advance_expression(value, name, prior_call);
            *prior_call = false;
            read_after
        }
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(_)) =>
        {
            // A chained scalar assignment is represented inside the outer
            // assignment's value (`a = (b = (c = 0))`).  The outer target is
            // not a read and has no evaluation effects, so retain expression
            // sequencing while looking for a nested definition of `name`.
            // Falling through to the generic read/call query would miss that
            // definition and incorrectly carry an earlier call across the
            // freshly assigned value.
            advance_expression(value, name, prior_call)
        }
        Expression::Comma { left, right } => {
            let left_read = advance_expression(left, name, prior_call);
            left_read | advance_expression(right, name, prior_call)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            let mut read_after = advance_expression(condition, name, prior_call);
            let mut true_call = *prior_call;
            let mut false_call = *prior_call;
            read_after |= advance_expression(when_true, name, &mut true_call);
            read_after |= advance_expression(when_false, name, &mut false_call);
            *prior_call = true_call || false_call;
            read_after
        }
        _ => {
            let read_after = expression_reads_name_across_call(expression, name, *prior_call);
            *prior_call |= expression_has_call(expression);
            read_after
        }
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

    fn immediate_result_guard(later_read: bool) -> Vec<Statement> {
        vec![
            Statement::Assign {
                name: "result".into(),
                value: Expression::Comma {
                    left: Box::new(Expression::IntegerLiteral(1)),
                    right: Box::new(Expression::Call {
                        name: "issue".into(),
                        arguments: Vec::new(),
                    }),
                },
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("result".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![Statement::Return(Some(Expression::IntegerLiteral(-1)))],
                else_body: Vec::new(),
            },
            Statement::Expression(if later_read {
                Expression::Variable("result".into())
            } else {
                Expression::Call {
                    name: "sleep".into(),
                    arguments: Vec::new(),
                }
            }),
        ]
    }

    fn assign(name: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(Expression::Variable(name.into())),
            value: Box::new(value),
        }
    }

    #[test]
    fn nested_comma_assignment_starts_a_fresh_lifetime_after_a_call() {
        let expression = Expression::Comma {
            left: Box::new(assign("alias", Expression::Variable("source".into()))),
            right: Box::new(Expression::Variable("alias".into())),
        };
        let mut prior_call = true;
        assert!(!advance_expression(&expression, "alias", &mut prior_call));
        assert!(!prior_call);
    }

    #[test]
    fn chained_scalar_assignment_starts_each_nested_lifetime_after_a_call() {
        let expression = assign(
            "outer",
            assign(
                "middle",
                assign("candidate", Expression::IntegerLiteral(0)),
            ),
        );
        let mut prior_call = true;

        assert!(!advance_expression(&expression, "candidate", &mut prior_call));
        assert!(!prior_call);
    }

    #[test]
    fn conditional_assignments_on_both_edges_start_a_fresh_lifetime() {
        let expression = Expression::Comma {
            left: Box::new(Expression::Conditional {
                condition: Box::new(Expression::Variable("condition".into())),
                when_true: Box::new(assign("result", Expression::IntegerLiteral(1))),
                when_false: Box::new(assign("result", Expression::IntegerLiteral(0))),
                origin: mwcc_syntax_trees::ConditionalOrigin::IfAssignments,
            }),
            right: Box::new(Expression::Variable("result".into())),
        };
        let mut prior_call = true;
        assert!(!advance_expression(&expression, "result", &mut prior_call));
        assert!(!prior_call);
    }

    #[test]
    fn conditional_assignment_on_one_edge_preserves_the_incoming_lifetime() {
        let expression = Expression::Comma {
            left: Box::new(Expression::Conditional {
                condition: Box::new(Expression::Variable("condition".into())),
                when_true: Box::new(assign("result", Expression::IntegerLiteral(1))),
                when_false: Box::new(Expression::IntegerLiteral(0)),
                origin: mwcc_syntax_trees::ConditionalOrigin::IfAssignments,
            }),
            right: Box::new(Expression::Variable("result".into())),
        };
        let mut prior_call = true;
        assert!(advance_expression(&expression, "result", &mut prior_call));
        assert!(prior_call);
    }

    fn inline_result_alias(later_read: bool) -> Vec<Statement> {
        vec![
            Statement::Assign {
                name: "outer".into(),
                value: Expression::Comma {
                    left: Box::new(Expression::IntegerLiteral(1)),
                    right: Box::new(Expression::Comma {
                        left: Box::new(Expression::Assign {
                            target: Box::new(Expression::Variable("temporary".into())),
                            value: Box::new(Expression::Call {
                                name: "issue".into(),
                                arguments: Vec::new(),
                            }),
                        }),
                        right: Box::new(Expression::Variable("temporary".into())),
                    }),
                },
            },
            Statement::Expression(if later_read {
                Expression::Variable("temporary".into())
            } else {
                Expression::Call {
                    name: "sleep".into(),
                    arguments: Vec::new(),
                }
            }),
        ]
    }

    #[test]
    fn immediate_call_result_zero_guard_is_volatile() {
        assert!(immediate_call_result_zero_guard(
            &immediate_result_guard(false),
            None,
            "result"
        ));
    }

    #[test]
    fn later_call_result_read_prevents_volatile_classification() {
        assert!(!immediate_call_result_zero_guard(
            &immediate_result_guard(true),
            None,
            "result"
        ));
    }

    #[test]
    fn inline_terminal_call_result_alias_is_volatile() {
        assert!(inline_terminal_call_result_alias(
            &inline_result_alias(false),
            None,
            "temporary"
        ));
    }

    #[test]
    fn guarded_transparent_terminal_alias_is_volatile() {
        let statements = vec![
            Statement::Assign {
                name: "outer".into(),
                value: Expression::Comma {
                    left: Box::new(Expression::Assign {
                        target: Box::new(Expression::Variable("temporary".into())),
                        value: Box::new(Expression::Call {
                            name: "issue".into(),
                            arguments: Vec::new(),
                        }),
                    }),
                    right: Box::new(Expression::Comma {
                        left: Box::new(Expression::Cast {
                            target_type: mwcc_syntax_trees::Type::Void,
                            operand: Box::new(Expression::IntegerLiteral(0)),
                        }),
                        right: Box::new(Expression::Variable("temporary".into())),
                    }),
                },
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("outer".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![Statement::Return(Some(Expression::IntegerLiteral(-1)))],
                else_body: Vec::new(),
            },
        ];

        assert!(immediate_call_result_zero_guard(&statements, None, "outer"));
        assert!(inline_terminal_call_result_alias(
            &statements,
            None,
            "temporary"
        ));
    }

    #[test]
    fn a_calling_terminal_projection_keeps_the_alias_live() {
        let statements = vec![Statement::Assign {
            name: "outer".into(),
            value: Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("temporary".into())),
                    value: Box::new(Expression::Call {
                        name: "issue".into(),
                        arguments: Vec::new(),
                    }),
                }),
                right: Box::new(Expression::Comma {
                    left: Box::new(Expression::Call {
                        name: "observe".into(),
                        arguments: Vec::new(),
                    }),
                    right: Box::new(Expression::Variable("temporary".into())),
                }),
            },
        }];

        assert!(!inline_terminal_call_result_alias(
            &statements,
            None,
            "temporary"
        ));
    }

    #[test]
    fn later_inline_alias_read_prevents_volatile_classification() {
        assert!(!inline_terminal_call_result_alias(
            &inline_result_alias(true),
            None,
            "temporary"
        ));
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
    fn a_store_base_survives_a_call_in_its_value() {
        let statements = vec![Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("this".into())),
                offset: 4,
                member_type: mwcc_syntax_trees::Type::StructPointer { element_size: 4 },
                index_stride: None,
            },
            value: Expression::Call {
                name: "allocate".into(),
                arguments: vec![],
            },
        }];

        assert!(read_after_possible_call(&statements, "this", false).read_after_call);
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
    fn a_backward_edge_does_not_resurrect_an_unread_consumed_local() {
        let statements = vec![
            Statement::Expression(Expression::Variable("result".into())),
            Statement::Label("loop".into()),
            call("sleep"),
            Statement::Goto("loop".into()),
        ];

        assert!(!read_after_possible_call(&statements, "result", false).read_after_call);
    }

    #[test]
    fn a_backward_edge_preserves_a_value_read_on_the_next_iteration() {
        let statements = vec![
            Statement::Label("loop".into()),
            Statement::Expression(Expression::Variable("value".into())),
            call("sleep"),
            Statement::Goto("loop".into()),
        ];

        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_makes_reads_in_its_arm_live_across_the_call() {
        let statements = vec![Statement::If {
            condition: Expression::Call {
                name: "test".into(),
                arguments: vec![],
            },
            then_body: vec![Statement::Expression(Expression::Variable(
                "value".into(),
            ))],
            else_body: vec![],
        }];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_result_used_in_its_arm_stays_volatile() {
        let statements = vec![Statement::If {
            condition: Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Less,
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("value".into())),
                    value: Box::new(Expression::Call {
                        name: "produce".into(),
                        arguments: vec![],
                    }),
                }),
                right: Box::new(Expression::FloatLiteral(1.0)),
            },
            then_body: vec![Statement::Expression(Expression::Variable("value".into()))],
            else_body: vec![],
        }];

        assert!(
            !read_after_possible_call(&statements, "value", false)
                .read_after_call
        );
    }

    #[test]
    fn a_condition_call_result_with_a_calling_sibling_needs_a_saved_home() {
        let statements = vec![Statement::If {
            condition: Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Less,
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("value".into())),
                    value: Box::new(Expression::Call {
                        name: "produce".into(),
                        arguments: vec![],
                    }),
                }),
                right: Box::new(Expression::Call {
                    name: "limit".into(),
                    arguments: vec![],
                }),
            },
            then_body: vec![Statement::Expression(Expression::Variable(
                "value".into(),
            ))],
            else_body: vec![],
        }];

        assert!(
            read_after_possible_call(&statements, "value", false)
                .read_after_call
        );
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
    fn comparison_assignments_kill_earlier_call_lifetimes() {
        let assignment = |name: &str, value| Expression::Assign {
            target: Box::new(Expression::Variable(name.into())),
            value: Box::new(value),
        };
        let statements = vec![
            call("before"),
            Statement::If {
                condition: Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Greater,
                    left: Box::new(assignment("computed", Expression::FloatLiteral(1.0))),
                    right: Box::new(assignment("limit", Expression::FloatLiteral(2.0))),
                },
                then_body: vec![],
                else_body: vec![],
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![
                    Expression::Variable("computed".into()),
                    Expression::Variable("limit".into()),
                ],
            }),
        ];

        assert!(!read_after_possible_call(&statements, "computed", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "limit", false).read_after_call);
    }

    #[test]
    fn comparison_self_update_retains_the_earlier_lifetime() {
        let statements = vec![
            call("before"),
            Statement::If {
                condition: Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Greater,
                    left: Box::new(Expression::Assign {
                        target: Box::new(Expression::Variable("value".into())),
                        value: Box::new(Expression::Binary {
                            operator: mwcc_syntax_trees::BinaryOperator::Add,
                            left: Box::new(Expression::Variable("value".into())),
                            right: Box::new(Expression::FloatLiteral(1.0)),
                        }),
                    }),
                    right: Box::new(Expression::FloatLiteral(2.0)),
                },
                then_body: vec![],
                else_body: vec![],
            },
        ];

        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_call_result_read_before_the_next_call_stays_volatile() {
        let statements = vec![
            Statement::Assign {
                name: "value".into(),
                value: Expression::Call {
                    name: "produce".into(),
                    arguments: vec![],
                },
            },
            Statement::Assign {
                name: "copy".into(),
                value: Expression::Variable("value".into()),
            },
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_call_result_read_after_another_call_needs_a_saved_home() {
        let statements = vec![
            Statement::Assign {
                name: "value".into(),
                value: Expression::Call {
                    name: "produce".into(),
                    arguments: vec![],
                },
            },
            call("intervening"),
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_do_while_final_call_result_is_fresh_at_the_loop_exit() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
                left: Box::new(Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::NotEqual,
                    left: Box::new(Expression::Variable("result".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                }),
                right: Box::new(Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Greater,
                    left: Box::new(Expression::Variable("tries".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                }),
            }),
            step: None,
            body: vec![
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::Call {
                        name: "send".into(),
                        arguments: vec![Expression::Variable("buffer".into())],
                    },
                },
                Statement::Assign {
                    name: "tries".into(),
                    value: Expression::Binary {
                        operator: mwcc_syntax_trees::BinaryOperator::Subtract,
                        left: Box::new(Expression::Variable("tries".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                },
            ],
        }];

        assert!(!read_after_possible_call_in_return(
            &statements,
            Some(&Expression::Variable("result".into())),
            "result",
        ));
        assert!(read_after_possible_call(&statements, "tries", false).read_after_call);
        assert!(read_after_possible_call(&statements, "buffer", false).read_after_call);
    }

    #[test]
    fn an_unrelated_indirect_call_does_not_resurrect_an_expired_value() {
        let statements = vec![
            call("before"),
            Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Expression(Expression::Variable("value".into())),
            Statement::Expression(Expression::CallThrough {
                target: Box::new(Expression::Variable("callback".into())),
                arguments: vec![Expression::Variable("object".into())],
            }),
        ];

        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_plain_indirect_call_reads_its_operands_before_the_branch() {
        let statements = vec![Statement::Expression(Expression::CallThrough {
            target: Box::new(Expression::Index {
                base: Box::new(Expression::Variable("callbacks".into())),
                index: Box::new(Expression::Variable("slot".into())),
            }),
            arguments: vec![Expression::Variable("object".into())],
        })];

        assert!(!read_after_possible_call(&statements, "slot", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "object", false).read_after_call);
    }

    #[test]
    fn a_nested_call_can_precede_an_indirect_call_operand_read() {
        let statements = vec![Statement::Expression(Expression::CallThrough {
            target: Box::new(Expression::Variable("callback".into())),
            arguments: vec![
                Expression::Variable("object".into()),
                Expression::Call {
                    name: "prepare".into(),
                    arguments: Vec::new(),
                },
            ],
        })];

        assert!(read_after_possible_call(&statements, "object", false).read_after_call);
    }

    #[test]
    fn direct_return_call_arguments_do_not_invent_saved_homes() {
        let tail = Expression::Call {
            name: "atan2f".into(),
            arguments: vec![Expression::Variable("object".into())],
        };

        assert!(!read_after_possible_call_in_return(
            &[],
            Some(&tail),
            "object"
        ));
    }

    #[test]
    fn an_initializer_call_makes_later_parameter_reads_survive() {
        let function = Function {
            return_type: mwcc_syntax_trees::Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![mwcc_syntax_trees::Parameter {
                parameter_type: mwcc_syntax_trees::Type::Pointer(
                    mwcc_syntax_trees::Pointee::UnsignedChar,
                ),
                name: "object".into(),
            }],
            locals: vec![mwcc_syntax_trees::LocalDeclaration {
                declared_type: mwcc_syntax_trees::Type::Int,
                name: "result".into(),
                initializer: Some(Expression::Call {
                    name: "inspect".into(),
                    arguments: vec![Expression::Variable("object".into())],
                }),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::Expression(Expression::Variable(
                "object".into(),
            ))],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert!(read_after_possible_call_in_function(&function, "object"));
        assert!(!read_after_possible_call_in_function(&function, "result"));
    }

    #[test]
    fn a_return_read_after_a_body_call_needs_a_saved_home() {
        let statements = vec![call("mutate")];
        let tail = Expression::Variable("object".into());

        assert!(read_after_possible_call_in_return(
            &statements,
            Some(&tail),
            "object"
        ));
    }

    #[test]
    fn a_for_counter_stepped_after_a_body_call_needs_a_saved_home() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Less,
                left: Box::new(Expression::Variable("index".into())),
                right: Box::new(Expression::IntegerLiteral(4)),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::Binary {
                    operator: mwcc_syntax_trees::BinaryOperator::Add,
                    left: Box::new(Expression::Variable("index".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body: vec![call("consume")],
        }];

        assert!(read_after_possible_call_in_return(
            &statements,
            None,
            "index"
        ));
    }

    #[test]
    fn a_loop_step_redefinition_kills_the_prior_call_lifetime() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Variable("index".into())),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            body: vec![call("consume")],
        }];

        assert!(!read_after_possible_call_in_return(
            &statements,
            None,
            "index"
        ));
    }

    #[test]
    fn a_call_argument_reused_on_the_next_iteration_needs_a_saved_home() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![Statement::Expression(Expression::Call {
                name: "compare".into(),
                arguments: vec![Expression::Variable("needle".into())],
            })],
        }];

        assert!(read_after_possible_call_in_return(
            &statements,
            None,
            "needle"
        ));
    }

    #[test]
    fn a_loop_redefinition_before_the_next_read_kills_the_backedge_call() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![
                call("consume"),
                Statement::Assign {
                    name: "value".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Expression(Expression::Variable("value".into())),
            ],
        }];

        assert!(!read_after_possible_call_in_return(
            &statements,
            None,
            "value"
        ));
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
