//! Conservative eligibility and alias-safety checks for AST inline expansion.

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement, Type};
use std::collections::HashSet;

pub(super) fn composable_function(function: &Function) -> bool {
    composable_function_with_assignable_parameters(function, false)
        && function
            .parameters
            .iter()
            .all(|parameter| !variable_is_modified_or_escaped(function, &parameter.name))
}

/// A reference-forwarding leaf can safely expose a parameter's source lvalue
/// to the one call it wraps. The frontend represents `const T& value` as a
/// pointer-shaped ABI parameter while source `&value` remains an
/// `AddressOf(Variable)` expression. That looks like an escaping parameter to
/// the general composer, but direct substitution is exact when the address is
/// the parameter's only use and the body has no other control flow.
pub(super) fn reference_forwarding_call_callee(function: &Function) -> bool {
    let [Statement::Expression(call @ Expression::Call { arguments, .. })] =
        function.statements.as_slice()
    else {
        return false;
    };
    if !composable_function_with_assignable_parameters(function, false) {
        return false;
    }
    let addressed = function
        .parameters
        .iter()
        .filter(|parameter| variable_is_modified_or_escaped(function, &parameter.name))
        .collect::<Vec<_>>();
    !addressed.is_empty()
        && addressed.iter().all(|parameter| {
            expression_use_count(call, &parameter.name) == 1
                && parameter_forwarded_by_address(arguments, &parameter.name)
        })
}

pub(super) fn parameter_forwarded_by_address(arguments: &[Expression], parameter: &str) -> bool {
    arguments.iter().any(|argument| {
        matches!(argument,
            Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == parameter))
    })
}

fn composable_function_with_assignable_parameters(
    function: &Function,
    parameters_are_assignable: bool,
) -> bool {
    let mut assignable_names: HashSet<&str> = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect();
    if parameters_are_assignable {
        assignable_names.extend(
            function
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str()),
        );
    }
    let discarded_result_is_safe = function.return_type == Type::Void
        || matches!(
            (
                function.parameters.first(),
                function.return_expression.as_ref()
            ),
            (
                Some(parameter),
                Some(Expression::Variable(result))
            ) if parameter.name == "this"
                && result == "this"
                && parameter.parameter_type == function.return_type
                && matches!(function.return_type, Type::StructPointer { .. })
        );
    discarded_result_is_safe
        && function.locals.iter().all(|local| {
            !local.is_static
                && !local.is_volatile
                && automatic_local_has_composable_storage(local)
                && (local.initializer.is_some()
                    || !matches!(local.declared_type, Type::Void | Type::Struct { .. }))
        })
        && uninitialized_local_reads_are_dominated(function)
        && function.guards.is_empty()
        && (function.return_expression.is_none()
            || matches!(function.return_expression, Some(Expression::Variable(ref name)) if name == "this"))
        && function.asm_body.is_none()
        && composable_statements(&function.statements, &assignable_names)
}

/// An inline instance gets an independently alpha-renamed declaration in the
/// caller. Fixed automatic arrays are therefore as hygienic as scalar locals,
/// provided their declaration is one the structured frame planner can
/// represent. Their contents are intentionally not treated as an
/// uninitialized scalar value: taking the array address or filling its
/// elements is the initialization.
fn automatic_local_has_composable_storage(
    local: &mwcc_syntax_trees::LocalDeclaration,
) -> bool {
    let Some(length) = local.array_length else {
        return true;
    };
    if length == 0
        || local.initializer.is_some()
        || !local.data_relocations.is_empty()
        || matches!(local.declared_type, Type::Void)
    {
        return false;
    }
    let element_bytes = match local.declared_type {
        Type::Struct { size, .. } => size,
        value_type => u32::from(value_type.width() / 8),
    };
    let Some(bytes) = element_bytes.checked_mul(u32::from(length)) else {
        return false;
    };
    bytes != 0
        && local
            .data_bytes
            .as_ref()
            .is_none_or(|image| image.len() <= bytes as usize)
}

/// Apply MWCC's small-body gate to ordinary one-call definitions newly made
/// composable by dominated, uninitialized locals. Explicit inline definitions
/// retain the broader semantic safety check above. Previously composable
/// initialized-local bodies also retain their established behavior.
pub(super) fn automatic_composable_function(function: &Function) -> bool {
    let ordinary = composable_function(function)
        && !function
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::Switch { .. }))
        && (function
            .locals
            .iter()
            .all(|local| local.initializer.is_some())
            || statement_weight(&function.statements) <= 6);
    let parameter_select = function.locals.is_empty()
        && function.return_type == Type::Void
        && function.return_expression.is_none()
        && function.parameters.iter().all(|parameter| {
            !matches!(parameter.parameter_type, Type::Void | Type::Struct { .. })
        })
        && automatic_parameter_select_store_body(function)
        && composable_function_with_assignable_parameters(function, true);
    ordinary || parameter_select
}

/// A leaf setter small enough for the 2.4.x automatic inliner to duplicate at
/// every source-visible call site. The complete scalar/member proof keeps the
/// repeatable lane from broadening to pointer escapes or compound updates.
pub(super) fn repeatable_scalar_member_setter_callee(function: &Function) -> bool {
    let [base, value] = function.parameters.as_slice() else {
        return false;
    };
    let [Statement::Store {
        target:
            Expression::Member {
                base: member_base,
                member_type,
                index_stride: None,
                ..
            },
        value: stored_value,
    }] = function.statements.as_slice()
    else {
        return false;
    };
    function.return_type == Type::Void
        && function.locals.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && matches!(base.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        && value.parameter_type == *member_type
        && !matches!(value.parameter_type, Type::Void | Type::Struct { .. })
        && matches!(member_base.as_ref(), Expression::Variable(name) if name == &base.name)
        && matches!(stored_value, Expression::Variable(name) if name == &value.name)
        && composable_function(function)
}

/// A one-use ordinary scalar helper that whole-file IPA can substitute as a
/// value expression without exposing caller-visible storage.
///
/// Keep this lane deliberately SSA-like: every uniform scalar local is
/// initialized exactly once, each initializer is pure, and the returned value
/// is one of those locals.  This covers small arithmetic helpers while keeping
/// aliasing, mutable parameters, and control flow in the statement composer.
pub(super) fn automatic_straight_line_scalar_value_function(function: &Function) -> bool {
    if matches!(function.return_type, Type::Void | Type::Struct { .. })
        || function.locals.is_empty()
        || function.locals.len() > 4
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || function
            .parameters
            .iter()
            .any(|parameter| {
                parameter.parameter_type != function.return_type
                    || variable_is_modified_or_escaped(function, &parameter.name)
            })
        || function.locals.iter().any(|local| {
            local.declared_type != function.return_type
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
                || local
                    .initializer
                    .as_ref()
                    .is_some_and(crate::analysis::expression_has_side_effect)
        })
        || !uninitialized_local_reads_are_dominated(function)
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name))
                if function.locals.iter().any(|local| local.name == *name)
        )
    {
        return false;
    }

    let local_names = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    let mut initialized = function
        .locals
        .iter()
        .filter(|local| local.initializer.is_some())
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    function.statements.iter().all(|statement| {
        let Statement::Assign { name, value } = statement else {
            return false;
        };
        local_names.contains(name.as_str())
            && initialized.insert(name)
            && !crate::analysis::expression_has_side_effect(value)
    }) && initialized.len() == local_names.len()
}

/// A bounded scalar transaction whose result is a local may be expanded as
/// statements at a call site even when its control flow cannot be represented
/// by the expression-summary lane. This admits a canonical queue-draining
/// `while ((item = pop())) consume(item);` loop while retaining the same
/// storage, dominance, parameter-alias, and control-flow safety proofs used by
/// ordinary statement-body composition.
pub(super) fn automatic_statement_value_function(function: &Function) -> bool {
    automatic_queue_draining_value_function(function)
        || automatic_guarded_accumulator_value_function(function)
        || automatic_conditional_local_value_function(function)
}

/// A source-visible scalar helper may keep its result in one initialized local
/// and select later values through a nested `if`/`else if` tree. Keeping this
/// as statements preserves the local's single captured image and avoids
/// duplicating parameter/member reads while converting the tree to a value
/// expression.
fn automatic_conditional_local_value_function(function: &Function) -> bool {
    let [result] = function.locals.as_slice() else {
        return false;
    };
    if matches!(function.return_type, Type::Void | Type::Struct { .. })
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        )
        || result.initializer.is_none()
        || result.is_static
        || result.is_volatile
        || !automatic_local_has_composable_storage(result)
        || matches!(result.declared_type, Type::Void | Type::Struct { .. })
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
        || statement_weight(&function.statements) > 16
        || !matches!(function.statements.as_slice(), [Statement::If { .. }])
        || function
            .parameters
            .iter()
            .any(|parameter| variable_is_modified_or_escaped(function, &parameter.name))
    {
        return false;
    }
    let local_names = HashSet::from([result.name.as_str()]);
    statement_value_statements_are_composable(&function.statements, &local_names)
        && multi_call_transaction_callee(function)
}

fn automatic_queue_draining_value_function(function: &Function) -> bool {
    if matches!(function.return_type, Type::Void | Type::Struct { .. })
        || function.locals.is_empty()
        || function.locals.len() > 4
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || statement_weight(&function.statements) > 16
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name))
                if function.locals.iter().any(|local| local.name == *name)
        )
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || !automatic_local_has_composable_storage(local)
                || matches!(local.declared_type, Type::Void | Type::Struct { .. })
        })
        || function
            .parameters
            .iter()
            .any(|parameter| variable_is_modified_or_escaped(function, &parameter.name))
        || !uninitialized_local_reads_are_dominated(function)
        || function
            .statements
            .iter()
            .filter(|statement| is_queue_draining_loop(statement))
            .count()
            != 1
    {
        return false;
    }
    let local_names = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect();
    statement_value_statements_are_composable(&function.statements, &local_names)
        && multi_call_transaction_callee(function)
}

/// A small scalar reduction may be duplicated at each source-visible call
/// site even when its result has an early-return encoding. This is the other
/// common statement-valued IPA shape beside queue draining: walk a linked
/// range, accumulate callback failures, fold one final status call, then map
/// the accumulator to a scalar success result.
fn automatic_guarded_accumulator_value_function(function: &Function) -> bool {
    let [loop_statement, trailing] = function.statements.as_slice() else {
        return false;
    };
    let [guard] = function.guards.as_slice() else {
        return false;
    };
    let Expression::Variable(result_name) = &guard.condition else {
        return false;
    };
    let Some(result_local) = function
        .locals
        .iter()
        .find(|local| local.name == *result_name)
    else {
        return false;
    };
    if matches!(function.return_type, Type::Void | Type::Struct { .. })
        || function.locals.len() > 4
        || function.asm_body.is_some()
        || result_local.initializer.is_none()
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || !automatic_local_has_composable_storage(local)
                || matches!(local.declared_type, Type::Void | Type::Struct { .. })
        })
        || function
            .parameters
            .iter()
            .any(|parameter| variable_is_modified_or_escaped(function, &parameter.name))
        || crate::analysis::expression_has_side_effect(&guard.value)
        || function
            .return_expression
            .as_ref()
            .is_none_or(crate::analysis::expression_has_side_effect)
        || !uninitialized_local_reads_are_dominated(function)
        || !accumulator_assignment_has_call(trailing, result_name)
        || !bounded_accumulator_loop(loop_statement, result_name, function)
    {
        return false;
    }
    true
}

fn accumulator_assignment(statement: &Statement, result_name: &str) -> bool {
    matches!(
        statement,
        Statement::Assign {
            name,
            value: Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                ..
            },
        } if name == result_name
            && matches!(left.as_ref(), Expression::Variable(value) if value == result_name)
    )
}

fn accumulator_assignment_has_call(statement: &Statement, result_name: &str) -> bool {
    accumulator_assignment(statement, result_name)
        && matches!(statement, Statement::Assign { value, .. }
            if crate::analysis::expression_has_call(value))
}

fn bounded_accumulator_loop(
    statement: &Statement,
    result_name: &str,
    function: &Function,
) -> bool {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Assign { target, .. }),
        condition: Some(condition),
        step: Some(Expression::Assign {
            target: step_target,
            ..
        }),
        body,
    } = statement
    else {
        return false;
    };
    let (Expression::Variable(iterator), Expression::Variable(step_iterator)) =
        (target.as_ref(), step_target.as_ref())
    else {
        return false;
    };
    iterator == step_iterator
        && iterator != result_name
        && function.locals.iter().any(|local| local.name == *iterator)
        && !crate::analysis::expression_has_call(condition)
        && matches!(body.as_slice(), [body_statement]
            if accumulator_assignment_has_call(body_statement, result_name))
}

fn is_queue_draining_loop(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition:
                Some(Expression::Assign {
                    target,
                    value,
                }),
            step: None,
            body,
        } if matches!(target.as_ref(), Expression::Variable(_))
            && matches!(value.as_ref(), Expression::Call { .. })
            && !body.is_empty()
    )
}

fn statement_value_statements_are_composable(
    statements: &[Statement],
    local_names: &HashSet<&str>,
) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. } | Statement::Expression(_) | Statement::InlineAsm(_) => true,
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statement_value_statements_are_composable(then_body, local_names)
                && statement_value_statements_are_composable(else_body, local_names)
        }
        Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition:
                Some(Expression::Assign {
                    target,
                    value: _,
                }),
            step: None,
            body,
        } => {
            matches!(target.as_ref(), Expression::Variable(name)
                if local_names.contains(name.as_str()))
                && statement_value_statements_are_composable(body, local_names)
        }
        Statement::Switch { arms, default, .. } => {
            arms.iter().all(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statement_value_statements_are_composable(body, local_names)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            }) && default.as_ref().is_none_or(|arm| match arm {
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statement_value_statements_are_composable(body, local_names)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            })
        }
        Statement::Return(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => false,
    })
}

/// A larger switch transaction is available only to the bounded-caller IPA
/// lane. Keeping it out of ordinary one-call composition prevents unrelated
/// state dispatchers from being duplicated merely because their top-level AST
/// is represented by one `Switch` statement.
pub(super) fn bounded_switch_transaction_callee(function: &Function) -> bool {
    function.is_static
        && function.return_type == Type::Void
        && function.parameters.is_empty()
        && function.locals.len() <= 1
        && matches!(function.statements.as_slice(), [Statement::Switch { .. }])
        && composable_function(function)
        && multi_call_transaction_callee(function)
}

/// A tiny guarded call transaction remains profitable at every source-visible
/// call site: the inlined condition can reuse caller values and removes one
/// otherwise unavoidable call/return boundary. Keep this distinct from plain
/// forwarding wrappers, which MWCC may leave out of line when referenced more
/// than once.
pub(super) fn repeatable_guarded_call_callee(function: &Function) -> bool {
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return false;
    };
    let [Statement::Expression(Expression::Call {
        arguments: guarded_arguments,
        ..
    })] = then_body.as_slice()
    else {
        return false;
    };
    if !else_body.is_empty()
        || !function.locals.is_empty()
        || !crate::analysis::expression_has_call(condition)
        || guarded_arguments
            .iter()
            .any(crate::analysis::expression_has_call)
        || !automatic_composable_function(function)
    {
        return false;
    }
    let mut calls = std::collections::HashMap::new();
    super::collect_function_calls(function, &mut calls);
    calls.values().sum::<usize>() == 2
}

/// A multi-use guarded transaction may still be inlined into terminal wrappers
/// even when it exceeds the ordinary tiny-body gate. Keep this classification
/// separate from general repeatable inlining: the caller must provide the
/// terminal-wrapper context before the body becomes available.
pub(super) fn repeatable_terminal_wrapper_callee(function: &Function) -> bool {
    let [Statement::If {
        then_body,
        else_body,
        ..
    }] = function.statements.as_slice()
    else {
        return false;
    };
    if !function.is_static
        || !else_body.is_empty()
        || then_body.is_empty()
        || statement_weight(&function.statements) > 8
        || !composable_function_with_assignable_parameters(function, true)
        || function.locals.iter().any(|local| {
            local.array_length.is_some()
                && crate::analysis::function_uses_name(function, &local.name)
        })
    {
        return false;
    }
    let mut calls = std::collections::HashMap::new();
    super::collect_function_calls(function, &mut calls);
    calls.values().sum::<usize>() >= 3
}

/// A compact repeated helper that sequences several calls is a transaction
/// worth duplicating even when a bounded caller contains only one invocation.
/// Keep this as a callee property so caller selection can remain concerned
/// solely with visibility and growth limits.
pub(super) fn multi_call_transaction_callee(function: &Function) -> bool {
    let mut calls = std::collections::HashMap::new();
    super::collect_function_calls(function, &mut calls);
    calls.values().sum::<usize>() >= 3
}

/// A one-use helper may treat scalar parameters as mutable local value lanes,
/// select among them through nested branches, and commit one final store. MWCC
/// expands this shape even when its branch weight exceeds the ordinary tiny-
/// body gate. The call-site composer materializes each modified parameter so
/// substitution cannot assign through the caller's argument expression.
fn automatic_parameter_select_store_body(function: &Function) -> bool {
    let Some((last, prefix)) = function.statements.split_last() else {
        return false;
    };
    matches!(last, Statement::Store { .. })
        && !prefix.is_empty()
        && statement_weight(&function.statements) <= 10
        && parameter_select_statements(prefix, function)
}

fn parameter_select_statements(statements: &[Statement], function: &Function) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { name, .. } => function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *name),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            parameter_select_statements(then_body, function)
                && parameter_select_statements(else_body, function)
        }
        _ => false,
    })
}

pub(super) fn statement_weight(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => 1 + statement_weight(then_body) + statement_weight(else_body),
            _ => 1,
        })
        .sum()
}

/// Prove that every read of an uninitialized scalar local is dominated by an
/// assignment on all incoming paths. This admits automatic-inline bodies that
/// express a select as `if/else` assignments without inventing an initial
/// value on a missing branch.
fn uninitialized_local_reads_are_dominated(function: &Function) -> bool {
    let tracked: HashSet<&str> = function
        .locals
        .iter()
        .filter(|local| local.initializer.is_none() && local.array_length.is_none())
        .map(|local| local.name.as_str())
        .collect();
    reads_are_dominated(&function.statements, &tracked, &mut HashSet::new())
}

fn reads_are_dominated<'a>(
    statements: &'a [Statement],
    tracked: &HashSet<&'a str>,
    assigned: &mut HashSet<&'a str>,
) -> bool {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } => {
                if reads_unassigned(value, tracked, assigned) {
                    return false;
                }
                if let Some(name) = tracked.get(name.as_str()) {
                    assigned.insert(*name);
                }
            }
            Statement::Store { target, value } => {
                if reads_unassigned(target, tracked, assigned)
                    || reads_unassigned(value, tracked, assigned)
                {
                    return false;
                }
            }
            Statement::Expression(expression) => {
                if reads_unassigned(expression, tracked, assigned) {
                    return false;
                }
            }
            Statement::InlineAsm(_) => {}
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                if reads_unassigned(condition, tracked, assigned) {
                    return false;
                }
                let mut then_assigned = assigned.clone();
                let mut else_assigned = assigned.clone();
                if !reads_are_dominated(then_body, tracked, &mut then_assigned)
                    || !reads_are_dominated(else_body, tracked, &mut else_assigned)
                {
                    return false;
                }
                assigned.retain(|name| {
                    then_assigned.contains(name) && else_assigned.contains(name)
                });
                assigned.extend(then_assigned.intersection(&else_assigned).copied());
            }
            Statement::Return(value) => {
                if value
                    .as_ref()
                    .is_some_and(|value| reads_unassigned(value, tracked, assigned))
                {
                    return false;
                }
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                if let Some(initializer) = initializer {
                    if !record_dominating_assignment(initializer, tracked, assigned) {
                        return false;
                    }
                }
                if let Some(condition) = condition {
                    if !record_dominating_assignment(condition, tracked, assigned) {
                        return false;
                    }
                }
                let mut loop_assigned = assigned.clone();
                if !reads_are_dominated(body, tracked, &mut loop_assigned)
                    || step
                        .as_ref()
                        .is_some_and(|step| reads_unassigned(step, tracked, &loop_assigned))
                {
                    return false;
                }
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                if reads_unassigned(scrutinee, tracked, assigned) {
                    return false;
                }
                let mut exits = Vec::with_capacity(arms.len() + usize::from(default.is_some()));
                for arm in arms {
                    let mwcc_syntax_trees::ArmBody::Statements(body) = &arm.body else {
                        return false;
                    };
                    let mut arm_assigned = assigned.clone();
                    if !reads_are_dominated(body, tracked, &mut arm_assigned) {
                        return false;
                    }
                    exits.push(arm_assigned);
                }
                if let Some(default) = default {
                    let mwcc_syntax_trees::ArmBody::Statements(body) = default else {
                        return false;
                    };
                    let mut default_assigned = assigned.clone();
                    if !reads_are_dominated(body, tracked, &mut default_assigned) {
                        return false;
                    }
                    exits.push(default_assigned);
                } else {
                    // The scrutinee can miss every case, preserving only values
                    // already assigned before the switch.
                    exits.push(assigned.clone());
                }
                assigned.retain(|name| exits.iter().all(|exit| exit.contains(name)));
            }
            Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => return false,
        }
    }
    true
}

fn record_dominating_assignment<'a>(
    expression: &'a Expression,
    tracked: &HashSet<&'a str>,
    assigned: &mut HashSet<&'a str>,
) -> bool {
    let Expression::Assign { target, value } = expression else {
        return !reads_unassigned(expression, tracked, assigned);
    };
    let Expression::Variable(name) = target.as_ref() else {
        return !reads_unassigned(expression, tracked, assigned);
    };
    if reads_unassigned(value, tracked, assigned) {
        return false;
    }
    if let Some(name) = tracked.get(name.as_str()) {
        assigned.insert(*name);
    }
    true
}

fn reads_unassigned(
    expression: &Expression,
    tracked: &HashSet<&str>,
    assigned: &HashSet<&str>,
) -> bool {
    tracked
        .iter()
        .any(|name| !assigned.contains(name) && expression_mentions(expression, name))
}

fn composable_statements(statements: &[Statement], local_names: &HashSet<&str>) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. } | Statement::Expression(_) | Statement::InlineAsm(_) => true,
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            composable_statements(then_body, local_names)
                && composable_statements(else_body, local_names)
        }
        Statement::Loop {
            initializer: Some(Expression::Assign { target, .. }),
            condition: Some(_),
            step: Some(step),
            body,
            ..
        } => {
            let Expression::Variable(counter) = target.as_ref() else {
                return false;
            };
            local_names.contains(counter.as_str())
                && loop_step_updates(step, counter)
                && composable_statements(body, local_names)
        }
        Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(0)),
            step: None,
            body,
        } => body.is_empty(),
        Statement::Switch { arms, default, .. } => {
            arms.iter().all(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    composable_statements(body, local_names)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            }) && default.as_ref().is_none_or(|arm| match arm {
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    composable_statements(body, local_names)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            })
        }
        // A void return is local control flow, not an escape from the caller.
        // Expansion rewrites it to a forward jump to the end of this particular
        // inline instance before the body enters instruction selection.
        Statement::Return(None) => true,
        Statement::Return(Some(_))
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => false,
    })
}

fn loop_step_updates(expression: &Expression, counter: &str) -> bool {
    match expression {
        Expression::PostStep { target, .. } => {
            matches!(target.as_ref(), Expression::Variable(name) if name == counter)
        }
        Expression::Assign { target, .. } => {
            matches!(target.as_ref(), Expression::Variable(name) if name == counter)
        }
        _ => false,
    }
}

pub(super) fn stable_argument(expression: &Expression, stable_variables: &HashSet<String>) -> bool {
    match expression {
        Expression::Variable(name) => stable_variables.contains(name),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => true,
        // A by-reference aggregate argument is represented as its aggregate
        // member lvalue rather than an explicit AddressOf. Scalarization may
        // read several declared fields from it, but the lvalue's address is
        // stable whenever its base is stable. Unions and unsupported aggregate
        // copies never reach composition because frontend scalarization declines
        // them.
        Expression::Member {
            member_type: Type::Struct { .. },
            ..
        } => stable_lvalue_address(expression, stable_variables),
        // An inherited non-virtual member call passes `this + base_offset`.
        // This address calculation is as stable and side-effect-free as its
        // complete-object base, so retained inline bodies may substitute it
        // without inventing a temporary or changing evaluation count.
        Expression::MemberAddress { base, .. } => stable_lvalue_address(base, stable_variables),
        // Taking an lvalue's address does not read or mutate the object. Repeating
        // a stable base/index calculation in an expanded setter therefore
        // preserves both its value and its evaluation count.
        Expression::AddressOf { operand } => stable_lvalue_address(operand, stable_variables),
        _ => false,
    }
}

pub(super) fn stable_lvalue_address(
    expression: &Expression,
    stable_variables: &HashSet<String>,
) -> bool {
    match expression {
        Expression::Variable(_) => true,
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            stable_argument(base, stable_variables)
        }
        Expression::Index { base, index } => {
            stable_argument(base, stable_variables) && stable_argument(index, stable_variables)
        }
        Expression::Dereference { pointer } => stable_argument(pointer, stable_variables),
        _ => false,
    }
}

/// Whether substituting call arguments into this retained body preserves
/// evaluation count. Stable scalar values are always safe. One otherwise
/// impure argument is also safe when a store-only setter/constructor consumes
/// it exactly once as a stored value: substitution neither duplicates nor
/// drops the evaluation. Other stores may initialize independent fields such
/// as a constructor's vptr.
pub(super) fn stable_arguments(
    function: &Function,
    arguments: &[Expression],
    stable_variables: &HashSet<String>,
) -> bool {
    if function.parameters.len() != arguments.len() {
        return false;
    }
    let unstable: Vec<usize> = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| {
            (!stable_argument(argument, stable_variables)).then_some(index)
        })
        .collect();
    if unstable.is_empty() {
        return true;
    }
    if reference_forwarding_call_callee(function)
        && unstable.iter().all(|index| {
            stable_lvalue_address(&arguments[*index], stable_variables)
                && !crate::analysis::expression_has_side_effect(&arguments[*index])
        })
    {
        return true;
    }
    // A verified one-store member setter consumes both parameters exactly
    // once.  A changing indexed address such as `&objects[i]` is therefore
    // safe to substitute directly when forming the store: it is neither
    // duplicated nor moved across another effect.  This is narrower than
    // treating changing lvalue addresses as generally stable.
    if repeatable_scalar_member_setter_callee(function)
        && matches!(arguments, [Expression::AddressOf { .. }, value]
            if stable_argument(value, stable_variables))
        && arguments
            .iter()
            .all(|argument| !crate::analysis::expression_has_side_effect(argument))
    {
        return true;
    }
    let [unstable_index] = unstable.as_slice() else {
        return false;
    };
    // A direct-return accessor with one use of the argument has no intervening
    // body effects and does not duplicate evaluation. This admits an automatic
    // local assigned earlier in the caller without treating all assigned locals
    // as globally stable across arbitrary inline bodies.
    if function.locals.is_empty() && function.statements.is_empty() {
        return function.return_expression.as_ref().is_some_and(|value| {
            expression_use_count(value, &function.parameters[*unstable_index].name) == 1
        });
    }
    let parameter = &function.parameters[*unstable_index].name;
    let stores: Option<Vec<_>> = function
        .statements
        .iter()
        .map(|statement| match statement {
            Statement::Store { target, value } => Some((target, value)),
            _ => None,
        })
        .collect();
    stores.is_some_and(|stores| {
        stores
            .iter()
            .all(|(target, _)| !expression_mentions(target, parameter))
            && stores
                .iter()
                .map(|(_, value)| expression_use_count(value, parameter))
                .sum::<usize>()
                == 1
    })
}

/// Whether arguments that cannot be substituted repeatedly may instead be
/// evaluated into hygienic scalar temporaries at the inline call site.
///
/// A scalar member read is side-effect-free but not intrinsically stable: the
/// callee might write the same storage between uses. Materializing it once
/// reproduces ordinary call argument semantics and lets statement-body
/// composition handle member-valued automatic-inline arguments safely.
pub(super) fn materializable_arguments(
    function: &Function,
    arguments: &[Expression],
    stable_variables: &HashSet<String>,
    allow_changing_scalars: bool,
) -> bool {
    let forwarded_reference_arguments = reference_forwarding_call_callee(function)
        .then(|| match function.statements.as_slice() {
            [Statement::Expression(Expression::Call { arguments, .. })] => {
                Some(arguments.as_slice())
            }
            _ => None,
        })
        .flatten();
    function.parameters.len() == arguments.len()
        && function
            .parameters
            .iter()
            .zip(arguments)
            .all(|(parameter, argument)| {
                stable_argument(argument, stable_variables)
                    || (forwarded_reference_arguments.is_some_and(|forwarded| {
                        parameter_forwarded_by_address(forwarded, &parameter.name)
                    }) && stable_lvalue_address(argument, stable_variables))
                    // A scalar local read is side-effect-free at the call site.
                    // Copying it into a hygienic inline parameter preserves the
                    // ordinary once-only argument evaluation even when that
                    // caller local is reassigned elsewhere in the function.
                    || (automatic_parameter_select_store_body(function)
                        && matches!(argument, Expression::Variable(_)))
                    || (allow_changing_scalars
                        && matches!(argument, Expression::Variable(_))
                        && !matches!(parameter.parameter_type, Type::Void | Type::Struct { .. }))
                    // A scalar-producing call is already evaluated exactly
                    // once for an ordinary call. Capturing its result in the
                    // hygienic parameter lane preserves that sequencing while
                    // allowing the expanded body to use the value later.
                    || (matches!(argument, Expression::Call { .. })
                        && !matches!(parameter.parameter_type, Type::Void | Type::Struct { .. }))
                    // Arithmetic expressions are evaluated once into the
                    // ordinary scalar argument lane. Capturing the complete
                    // expression—not only a bare nested call—preserves a
                    // source argument such as `magnitude() / length` before
                    // the retained body consumes it.
                    || (forwarded_reference_arguments.is_some()
                        && matches!(
                            parameter.parameter_type,
                            Type::Int
                                | Type::UnsignedInt
                                | Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                                | Type::Float
                                | Type::Double
                                | Type::LongLong
                                | Type::UnsignedLongLong
                        ))
                    || matches!(
                        argument,
                        Expression::Member {
                            base,
                            member_type,
                            index_stride: None,
                            ..
                        } if !matches!(member_type, Type::Void | Type::Struct { .. })
                            && (stable_argument(base, stable_variables)
                                || matches!(base.as_ref(), Expression::Variable(_)))
                    )
                    // A scalar array element is the indexed counterpart to a
                    // member read above. Capture the selected value once at
                    // the call site; this preserves volatile reads and keeps
                    // the expanded body from re-evaluating either operand.
                    || matches!(
                        argument,
                        Expression::Index { base, index }
                            if (stable_argument(base, stable_variables)
                                || matches!(base.as_ref(), Expression::Variable(_)))
                                && stable_argument(index, stable_variables)
                    )
            })
}

/// A terminal void call may reuse caller scalar variables as the callee's
/// parameter lanes, including parameters reassigned by the callee. No caller
/// statement or return expression can observe the overwritten local identity.
pub(super) fn terminal_scalar_arguments(
    function: &Function,
    arguments: &[Expression],
    stable_variables: &HashSet<String>,
) -> bool {
    function.return_type == Type::Void
        && function.return_expression.is_none()
        && function.parameters.len() == arguments.len()
        && function
            .parameters
            .iter()
            .zip(arguments)
            .all(|(parameter, argument)| {
                stable_argument(argument, stable_variables)
                    || (matches!(argument, Expression::Variable(_))
                        && !matches!(parameter.parameter_type, Type::Void | Type::Struct { .. }))
            })
}

pub(super) fn expression_use_count(expression: &Expression, name: &str) -> usize {
    match expression {
        Expression::Variable(variable) => usize::from(variable == name),
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .map(|element| expression_use_count(element, name))
            .sum(),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            expression_use_count(left, name) + expression_use_count(right, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_use_count(condition, name)
                + expression_use_count(when_true, name)
                + expression_use_count(when_false, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => expression_use_count(operand, name),
        Expression::Index { base, index } => {
            expression_use_count(base, name) + expression_use_count(index, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_use_count(base, name)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .map(|argument| expression_use_count(argument, name))
            .sum(),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            expression_use_count(allocation, name)
                + arguments
                    .iter()
                    .map(|argument| expression_use_count(argument, name))
                    .sum::<usize>()
        }
        Expression::CallThrough { target, arguments } => {
            expression_use_count(target, name)
                + arguments
                    .iter()
                    .map(|argument| expression_use_count(argument, name))
                    .sum::<usize>()
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            expression_use_count(object, name)
                + arguments
                    .iter()
                    .map(|argument| expression_use_count(argument, name))
                    .sum::<usize>()
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. } => 0,
    }
}

/// Values whose address never escapes and which are never reassigned cannot be
/// changed by an intervening statement from an expanded body. Substituting
/// them therefore preserves the call-time value without inventing an AST local
/// (which would incorrectly leak a compiler temporary into debug information).
pub(super) fn stable_local_values(function: &Function) -> HashSet<String> {
    if function.asm_body.is_some() {
        return HashSet::new();
    }
    function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .filter(|name| !variable_is_modified_or_escaped(function, name))
        .map(str::to_owned)
        .collect()
}

fn variable_is_modified_or_escaped(function: &Function, name: &str) -> bool {
    function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
        .any(|expression| expression_modifies_or_escapes(expression, name))
        || function.guards.iter().any(|guard| {
            expression_modifies_or_escapes(&guard.condition, name)
                || expression_modifies_or_escapes(&guard.value, name)
        })
        || function
            .return_expression
            .as_ref()
            .is_some_and(|expression| expression_modifies_or_escapes(expression, name))
        || function
            .statements
            .iter()
            .any(|statement| statement_modifies_or_escapes(statement, name))
}

pub(super) fn parameter_requires_materialization(function: &Function, name: &str) -> bool {
    variable_is_modified_or_escaped(function, name)
}

fn statement_modifies_or_escapes(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::InlineAsm(_) => false,
        Statement::Store { target, value } => {
            matches!(target, Expression::Variable(target_name) if target_name == name)
                || expression_modifies_or_escapes(target, name)
                || expression_modifies_or_escapes(value, name)
        }
        Statement::Assign {
            name: target_name,
            value,
        } => target_name == name || expression_modifies_or_escapes(value, name),
        Statement::Expression(expression) => expression_modifies_or_escapes(expression, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_modifies_or_escapes(condition, name)
                || then_body
                    .iter()
                    .any(|statement| statement_modifies_or_escapes(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_modifies_or_escapes(statement, name))
        }
        Statement::Return(expression) => expression
            .as_ref()
            .is_some_and(|expression| expression_modifies_or_escapes(expression, name)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_modifies_or_escapes(scrutinee, name)
                || arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(expression) => {
                        expression_modifies_or_escapes(expression, name)
                    }
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_modifies_or_escapes(statement, name)),
                })
                || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(expression) => {
                        expression_modifies_or_escapes(expression, name)
                    }
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_modifies_or_escapes(statement, name)),
                })
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .is_some_and(|expression| expression_modifies_or_escapes(expression, name))
                || condition
                    .as_ref()
                    .is_some_and(|expression| expression_modifies_or_escapes(expression, name))
                || step
                    .as_ref()
                    .is_some_and(|expression| expression_modifies_or_escapes(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_modifies_or_escapes(statement, name))
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
    }
}

fn expression_modifies_or_escapes(expression: &Expression, name: &str) -> bool {
    match expression {
        // `&local` exposes the local object's storage. `&pointer->member` only
        // exposes the pointee; it cannot change the pointer value substituted
        // into a retained inline body.
        Expression::AddressOf { operand } => {
            matches!(operand.as_ref(), Expression::Variable(variable) if variable == name)
        }
        Expression::PostStep {
            target: operand, ..
        } => matches!(operand.as_ref(), Expression::Variable(variable) if variable == name),
        Expression::Assign { target, value } => {
            matches!(target.as_ref(), Expression::Variable(variable) if variable == name)
                || expression_modifies_or_escapes(target, name)
                || expression_modifies_or_escapes(value, name)
        }
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .any(|element| expression_modifies_or_escapes(element, name)),
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            expression_modifies_or_escapes(left, name)
                || expression_modifies_or_escapes(right, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_modifies_or_escapes(condition, name)
                || expression_modifies_or_escapes(when_true, name)
                || expression_modifies_or_escapes(when_false, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand } => {
            expression_modifies_or_escapes(operand, name)
        }
        Expression::Index { base, index } => {
            expression_modifies_or_escapes(base, name)
                || expression_modifies_or_escapes(index, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_modifies_or_escapes(base, name)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_modifies_or_escapes(argument, name)),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            expression_modifies_or_escapes(allocation, name)
                || arguments
                    .iter()
                    .any(|argument| expression_modifies_or_escapes(argument, name))
        }
        Expression::CallThrough { target, arguments } => {
            expression_modifies_or_escapes(target, name)
                || arguments
                    .iter()
                    .any(|argument| expression_modifies_or_escapes(argument, name))
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            expression_modifies_or_escapes(object, name)
                || arguments
                    .iter()
                    .any(|argument| expression_modifies_or_escapes(argument, name))
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

fn expression_mentions(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Variable(variable) => variable == name,
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .any(|element| expression_mentions(element, name)),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            expression_mentions(left, name) || expression_mentions(right, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_mentions(condition, name)
                || expression_mentions(when_true, name)
                || expression_mentions(when_false, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => expression_mentions(operand, name),
        Expression::Index { base, index } => {
            expression_mentions(base, name) || expression_mentions(index, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_mentions(base, name)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_mentions(argument, name)),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            expression_mentions(allocation, name)
                || arguments
                    .iter()
                    .any(|argument| expression_mentions(argument, name))
        }
        Expression::CallThrough { target, arguments } => {
            expression_mentions(target, name)
                || arguments
                    .iter()
                    .any(|argument| expression_mentions(argument, name))
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            expression_mentions(object, name)
                || arguments
                    .iter()
                    .any(|argument| expression_mentions(argument, name))
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{
        ArmBody, BinaryOperator, LocalDeclaration, Parameter, SwitchArm,
    };

    fn scalar_parameter_function() -> Function {
        Function {
            return_type: Type::Void,
            name: "consume".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_a_repeatable_scalar_member_setter() {
        let mut function = scalar_parameter_function();
        function.parameters.insert(
            0,
            Parameter {
                parameter_type: Type::StructPointer { element_size: 32 },
                name: "record".into(),
            },
        );
        function.statements.push(Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("record".into())),
                offset: 4,
                member_type: Type::Int,
                index_stride: None,
            },
            value: Expression::Variable("value".into()),
        });

        assert!(repeatable_scalar_member_setter_callee(&function));
        function.statements.push(Statement::Expression(Expression::Call {
            name: "observe".into(),
            arguments: Vec::new(),
        }));
        assert!(!repeatable_scalar_member_setter_callee(&function));
    }

    #[test]
    fn admits_a_changing_indexed_address_for_a_one_store_setter() {
        let mut function = scalar_parameter_function();
        function.parameters.insert(
            0,
            Parameter {
                parameter_type: Type::StructPointer { element_size: 32 },
                name: "record".into(),
            },
        );
        function.statements.push(Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("record".into())),
                offset: 4,
                member_type: Type::Int,
                index_stride: None,
            },
            value: Expression::Variable("value".into()),
        });
        let arguments = [
            Expression::AddressOf {
                operand: Box::new(Expression::Index {
                    base: Box::new(Expression::Variable("records".into())),
                    index: Box::new(Expression::Variable("i".into())),
                }),
            },
            Expression::IntegerLiteral(0),
        ];

        assert!(stable_arguments(&function, &arguments, &HashSet::new()));
    }

    #[test]
    fn materializes_scalar_member_from_a_changing_local_once() {
        let argument = Expression::Member {
            base: Box::new(Expression::Variable("record".into())),
            offset: 4,
            member_type: Type::Int,
            index_stride: None,
        };

        assert!(materializable_arguments(
            &scalar_parameter_function(),
            &[argument],
            &HashSet::new(),
            false,
        ));
    }

    #[test]
    fn materializes_a_constant_index_from_a_named_array_once() {
        let argument = Expression::Index {
            base: Box::new(Expression::Variable("registers".into())),
            index: Box::new(Expression::IntegerLiteral(8)),
        };

        assert!(materializable_arguments(
            &scalar_parameter_function(),
            &[argument],
            &HashSet::new(),
            false,
        ));
    }

    #[test]
    fn does_not_materialize_a_member_with_an_effectful_base() {
        let argument = Expression::Member {
            base: Box::new(Expression::Call {
                name: "record".into(),
                arguments: Vec::new(),
            }),
            offset: 4,
            member_type: Type::Int,
            index_stride: None,
        };

        assert!(!materializable_arguments(
            &scalar_parameter_function(),
            &[argument],
            &HashSet::new(),
            false,
        ));
    }

    #[test]
    fn admits_a_one_use_interrupt_guarded_flag_transaction() {
        let mut function = scalar_parameter_function();
        function.parameters.clear();
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "level".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        function.statements = vec![
            Statement::Assign {
                name: "level".into(),
                value: Expression::Call {
                    name: "disable".into(),
                    arguments: Vec::new(),
                },
            },
            Statement::Store {
                target: Expression::Variable("pause".into()),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("executing".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![
                    Statement::Store {
                        target: Expression::Variable("pausing".into()),
                        value: Expression::IntegerLiteral(1),
                    },
                    Statement::Expression(Expression::Call {
                        name: "resume".into(),
                        arguments: Vec::new(),
                    }),
                ],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "restore".into(),
                arguments: vec![Expression::Variable("level".into())],
            }),
        ];

        assert_eq!(statement_weight(&function.statements), 6);
        assert!(automatic_composable_function(&function));
    }

    #[test]
    fn admits_a_guarded_multi_call_transaction_for_terminal_wrappers() {
        let mut function = scalar_parameter_function();
        function.name = "transaction".into();
        function.is_static = true;
        function.locals.push(LocalDeclaration {
            declared_type: Type::UnsignedChar,
            name: "scratch".into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(16),
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        function.statements = vec![Statement::If {
            condition: Expression::Call {
                name: "guard".into(),
                arguments: Vec::new(),
            },
            then_body: vec![
                Statement::Loop {
                    kind: mwcc_syntax_trees::LoopKind::DoWhile,
                    initializer: None,
                    condition: Some(Expression::IntegerLiteral(0)),
                    step: None,
                    body: Vec::new(),
                },
                Statement::Expression(Expression::Call {
                    name: "first".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "second".into(),
                    arguments: Vec::new(),
                }),
            ],
            else_body: Vec::new(),
        }];

        assert!(repeatable_terminal_wrapper_callee(&function));
    }

    #[test]
    fn reserves_a_multi_call_switch_for_bounded_caller_ipa() {
        let mut function = scalar_parameter_function();
        function.name = "switch_transaction".into();
        function.is_static = true;
        function.parameters.clear();
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "saved".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        function.statements = vec![Statement::Switch {
            scrutinee: Expression::Variable("command".into()),
            arms: vec![SwitchArm {
                value: 1,
                body: ArmBody::Statements(vec![
                    Statement::Assign {
                        name: "saved".into(),
                        value: Expression::Variable("current".into()),
                    },
                    Statement::Expression(Expression::Call {
                        name: "first".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Expression(Expression::Call {
                        name: "second".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Expression(Expression::Call {
                        name: "publish".into(),
                        arguments: vec![Expression::Variable("saved".into())],
                    }),
                ]),
                falls_through: false,
            }],
            default: None,
        }];

        assert!(bounded_switch_transaction_callee(&function));
        assert!(!automatic_composable_function(&function));
    }

    #[test]
    fn admits_a_loop_bearing_statement_value_transaction() {
        let mut function = scalar_parameter_function();
        function.name = "drain".into();
        function.return_type = Type::Int;
        let local = |name: &str| LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        };
        function.locals = vec![local("enabled"), local("item"), local("result")];
        function.statements = vec![
            Statement::Assign {
                name: "enabled".into(),
                value: Expression::Call {
                    name: "disable".into(),
                    arguments: Vec::new(),
                },
            },
            Statement::Loop {
                kind: mwcc_syntax_trees::LoopKind::While,
                initializer: None,
                condition: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("item".into())),
                    value: Box::new(Expression::Call {
                        name: "pop".into(),
                        arguments: Vec::new(),
                    }),
                }),
                step: None,
                body: vec![Statement::Expression(Expression::Call {
                    name: "cancel".into(),
                    arguments: vec![Expression::Variable("item".into())],
                })],
            },
            Statement::Assign {
                name: "result".into(),
                value: Expression::IntegerLiteral(1),
            },
        ];
        function.return_expression = Some(Expression::Variable("result".into()));

        assert!(automatic_statement_value_function(&function));
        assert!(!automatic_composable_function(&function));
    }
}
