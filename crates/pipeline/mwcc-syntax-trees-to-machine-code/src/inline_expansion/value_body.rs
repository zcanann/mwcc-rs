//! Expression summaries for retained non-void inline functions.
//!
//! A call embedded in a condition cannot accept statement splicing without
//! changing short-circuit evaluation. This module recognizes the common
//! `result = A; if (condition) result = B; return result;` body and preserves
//! it as a comma/conditional expression at the original call position.

use super::safety::composable_function;
use mwcc_syntax_trees::{
    BinaryOperator, ConditionalOrigin, Expression, Function, Statement, Type, UnaryOperator,
};

#[derive(Clone, Debug)]
pub(super) struct ValueInlineBody {
    pub(super) source: Function,
    pub(super) expression: Expression,
    pub(super) automatic_transaction: bool,
}

impl ValueInlineBody {
    pub(super) fn stores_global_name(&self, name: &str) -> bool {
        statements_store_global_name(&self.source.statements, name)
    }

    fn forwarded_call_arguments(&self) -> Option<&[Expression]> {
        match &self.expression {
            Expression::Call { arguments, .. } => Some(arguments),
            Expression::Comma { left, right }
                if matches!(right.as_ref(), Expression::IntegerLiteral(0)) =>
            {
                match left.as_ref() {
                    Expression::Call { arguments, .. } => Some(arguments),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether substituting caller arguments directly preserves both their
    /// single evaluation and source order. A pure forwarding wrapper uses
    /// every parameter exactly once, in its original position, so changing or
    /// side-effecting arguments do not need hygienic temporaries.
    pub(super) fn arguments_forwarded_once_in_order(&self) -> bool {
        let Some(forwarded) = self.forwarded_call_arguments() else {
            return false;
        };
        forwarded.len() == self.source.parameters.len()
            && forwarded
                .iter()
                .zip(&self.source.parameters)
                .all(|(argument, parameter)| {
                    matches!(argument, Expression::Variable(name) if name == &parameter.name)
                })
    }

    /// A pure caller expression can be substituted directly when the wrapper
    /// is one call and consumes this parameter exactly once. With no wrapper
    /// side effect before that call, materializing a compiler-only temporary
    /// would preserve semantics but lose MWCC's forwarding schedule.
    pub(super) fn parameter_used_once_in_forwarded_call(&self, name: &str) -> bool {
        self.forwarded_call_arguments().is_some_and(|arguments| {
            arguments
                .iter()
                .map(|argument| super::safety::expression_use_count(argument, name))
                .sum::<usize>()
                == 1
        })
    }

    /// Whether this larger transaction uses the diagnostic-bearing automatic
    /// inline lane. MWCC keeps a known function designator symbolic in this
    /// lane instead of extending its lifetime through an argument temporary.
    pub(super) fn forwards_known_function_designators(&self) -> bool {
        self.automatic_transaction && extended_diagnostic_transaction(&self.source)
    }
}

fn statements_store_global_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Store {
            target: Expression::Variable(target),
            ..
        } => target == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statements_store_global_name(then_body, name)
                || statements_store_global_name(else_body, name)
        }
        Statement::Loop { body, .. } => statements_store_global_name(body, name),
        Statement::Switch {
            arms, default, ..
        } => {
            arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statements_store_global_name(body, name)
                }
            }) || default.as_ref().is_some_and(|body| match body {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statements_store_global_name(body, name)
                }
            })
        }
        _ => false,
    })
}

pub(super) fn summarize(function: &Function) -> Option<ValueInlineBody> {
    if function.asm_body.is_some() {
        return None;
    }
    if function.return_type == Type::Void {
        if !function.guards.is_empty()
            || function.return_expression.is_some()
            || (!composable_function(function) && !sequenced_aggregate_void_body(function))
            || !function.statements.iter().all(void_expression_statement)
        {
            return None;
        }
        return summarize_sequenced_body(function, Expression::IntegerLiteral(0)).map(
            |expression| ValueInlineBody {
                source: function.clone(),
                expression,
                automatic_transaction: false,
            },
        );
    }
    if let Some(expression) = summarize_guard_chain(function) {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression,
            automatic_transaction: false,
        });
    }
    if let Some(expression) = summarize_conditional_return(function) {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression,
            automatic_transaction: false,
        });
    }
    // A direct scalar/member return is the smallest value-inline body. Keep it
    // before the result-local pattern below: ordinary (non-inline) definitions
    // use this shape too, and mwcc's automatic inliner substitutes sufficiently
    // small accessors while still emitting their external definition.
    if function.locals.is_empty() && function.statements.is_empty() {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression: normalize_reference_result(
                function.return_type,
                function.return_expression.clone()?,
            ),
            automatic_transaction: false,
        });
    }
    if let Some(expression) = summarize_result_selection(function) {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression,
            automatic_transaction: false,
        });
    }
    summarize_sequenced_body(
        function,
        normalize_reference_result(
            function.return_type,
            function.return_expression.clone()?,
        ),
    )
    .map(|expression| {
        ValueInlineBody {
            source: function.clone(),
            expression,
            automatic_transaction: false,
        }
    })
}

/// Preserve a retained inline's ordered `if (condition) return value;` chain.
///
/// These guards are already a value-selection DAG: lowering them to nested
/// conditionals keeps short-circuit order and leaves each chosen return
/// expression at its original control-flow point. Restrict this summary to a
/// body with no locals or ordinary statements so there are no side effects to
/// move across the first condition.
fn summarize_guard_chain(function: &Function) -> Option<Expression> {
    if function.guards.is_empty()
        || !function.locals.is_empty()
        || !function.statements.is_empty()
    {
        return None;
    }
    let fallback = normalize_reference_result(
        function.return_type,
        function.return_expression.clone()?,
    );
    Some(function.guards.iter().rev().fold(fallback, |otherwise, guard| {
        Expression::Conditional {
            condition: Box::new(guard.condition.clone()),
            when_true: Box::new(normalize_reference_result(
                function.return_type,
                guard.value.clone(),
            )),
            when_false: Box::new(otherwise),
            origin: ConditionalOrigin::IfReturns,
        }
    }))
}

/// Preserve a retained inline whose value is selected by one source-level
/// `if`, including effects local to either return arm.
///
/// A helper such as
///
/// `if (flag) { flag = 0; return 1; } else { return 0; }`
///
/// cannot be represented by the guard-only summary: the clear must remain
/// conditional. A conditional containing comma-sequenced arm effects keeps
/// that ownership explicit until call-site composition can either splice it
/// into an enclosing branch or lower it as an ordinary value diamond.
fn summarize_conditional_return(function: &Function) -> Option<Expression> {
    if !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    Some(Expression::Conditional {
        condition: Box::new(condition.clone()),
        when_true: Box::new(summarize_return_arm(then_body, function.return_type)?),
        when_false: Box::new(summarize_return_arm(else_body, function.return_type)?),
        origin: ConditionalOrigin::IfReturns,
    })
}

fn summarize_return_arm(statements: &[Statement], return_type: Type) -> Option<Expression> {
    let (last, effects) = statements.split_last()?;
    let Statement::Return(Some(result)) = last else {
        return None;
    };
    let mut expressions = effects
        .iter()
        .map(statement_expression)
        .collect::<Option<Vec<_>>>()?;
    expressions.push(normalize_reference_result(return_type, result.clone()));
    Some(sequence(expressions))
}

/// Preserve the address-valued result of a C++ reference accessor.
///
/// References use the pointer ABI type in the syntax tree, while their return
/// expression remains the referenced aggregate lvalue. A non-inlined call
/// communicates the pointer result through its signature; an inline summary
/// must make that address conversion explicit before the call node disappears.
fn normalize_reference_result(return_type: Type, result: Expression) -> Expression {
    if matches!(return_type, Type::StructPointer { .. })
        && matches!(
            result,
            Expression::Member {
                member_type: Type::Struct { .. },
                index_stride: None,
                ..
            }
        )
    {
        Expression::AddressOf {
            operand: Box::new(result),
        }
    } else {
        result
    }
}

fn sequenced_aggregate_void_body(function: &Function) -> bool {
    let local_names = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    function.locals.iter().any(|local| {
        local.initializer.is_none() && matches!(local.declared_type, Type::Struct { .. })
    }) && function.locals.iter().all(|local| {
        !local.is_static && !local.is_volatile && local.array_length.is_none()
    }) && assignments_target_only_locals(&function.statements, &local_names)
}

fn assignments_target_only_locals(
    statements: &[Statement],
    local_names: &std::collections::HashSet<&str>,
) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            assignments_target_only_locals(then_body, local_names)
                && assignments_target_only_locals(else_body, local_names)
        }
        _ => true,
    })
}

fn summarize_result_selection(function: &Function) -> Option<Expression> {
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if result.is_static
        || result.is_volatile
        || result.array_length.is_some()
        || result.initializer.is_some()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        )
    {
        return None;
    }
    let (prefix, tail) = function
        .statements
        .split_at(function.statements.len().saturating_sub(2));
    if !prefix
        .iter()
        .all(|statement| matches!(statement, Statement::Expression(_)))
    {
        return None;
    }
    let [Statement::Assign {
        name: initial_name,
        value: Expression::IntegerLiteral(initial),
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }] = tail
    else {
        return None;
    };
    let [Statement::Assign {
        name: selected_name,
        value: Expression::IntegerLiteral(selected),
    }] = then_body.as_slice()
    else {
        return None;
    };
    if initial_name != &result.name || selected_name != &result.name || !else_body.is_empty() {
        return None;
    }

    let selection = if *initial == 0 && *selected == 1 && is_boolean_expression(condition) {
        condition.clone()
    } else {
        Expression::Conditional {
            condition: Box::new(condition.clone()),
            when_true: Box::new(Expression::IntegerLiteral(*selected)),
            when_false: Box::new(Expression::IntegerLiteral(*initial)),
            origin: ConditionalOrigin::IfAssignments,
        }
    };
    let expression = prefix.iter().rev().fold(selection, |right, statement| {
        let Statement::Expression(left) = statement else {
            unreachable!("prefix eligibility checked")
        };
        Expression::Comma {
            left: Box::new(left.clone()),
            right: Box::new(right),
        }
    });
    Some(expression)
}

/// Convert a scalar inline body into a comma expression. Caller-owned fresh
/// locals are allocated when this summary is substituted, so initializers and
/// side effects still execute exactly where the original call appeared.
fn summarize_sequenced_body(function: &Function, result: Expression) -> Option<Expression> {
    summarize_sequenced_body_with_policy(function, result, false)
}

fn summarize_sequenced_body_with_policy(
    function: &Function,
    result: Expression,
    forward_terminal_result: bool,
) -> Option<Expression> {
    if function.locals.len() > 8
        || statement_count(&function.statements) > 12
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let mut expressions = Vec::new();
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            expressions.push(Expression::Assign {
                target: Box::new(Expression::Variable(local.name.clone())),
                value: Box::new(initializer.clone()),
            });
        }
    }
    let forwarded = forward_terminal_result
        .then(|| forwarded_terminal_local_result(function, &result))
        .flatten();
    let (statements, result) = forwarded.unwrap_or((&function.statements, result));
    for statement in statements {
        expressions.push(statement_expression(statement)?);
    }
    expressions.push(result);
    Some(sequence(expressions))
}

/// Forward a single terminal local assignment into the value summary.
///
/// Macro-expanded SDK sources commonly leave `(void)0;` between
/// `result = call();` and `return result;`. The local has no source-level
/// lifetime beyond carrying the call result, so retaining it after inlining
/// invents an extra value lane and move. Forward only an uninitialized local
/// whose terminal assignment is its sole mention in the statement prefix.
fn forwarded_terminal_local_result<'a>(
    function: &'a Function,
    result: &Expression,
) -> Option<(&'a [Statement], Expression)> {
    let Expression::Variable(result_name) = result else {
        return None;
    };
    let local = function
        .locals
        .iter()
        .find(|local| local.name == *result_name)?;
    if local.initializer.is_some() {
        return None;
    }
    let mut assignment_end = function.statements.len();
    while assignment_end > 0
        && inert_integer_void_statement(&function.statements[assignment_end - 1])
    {
        assignment_end -= 1;
    }
    let assignment_index = assignment_end.checked_sub(1)?;
    let Statement::Assign { name, value } = &function.statements[assignment_index] else {
        return None;
    };
    if name != result_name
        || super::safety::expression_use_count(value, result_name) != 0
        || function.statements[..assignment_index]
            .iter()
            .any(|statement| statement_mentions_name(statement, result_name))
    {
        return None;
    }
    Some((&function.statements[..assignment_index], value.clone()))
}

fn statement_mentions_name(statement: &Statement, name: &str) -> bool {
    let expression_mentions =
        |expression: &Expression| super::safety::expression_use_count(expression, name) != 0;
    match statement {
        Statement::InlineAsm(_) => false,
        Statement::Store { target, value } => {
            expression_mentions(target) || expression_mentions(value)
        }
        Statement::Assign {
            name: target,
            value,
        } => target == name || expression_mentions(value),
        Statement::Expression(value) => expression_mentions(value),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_mentions(condition)
                || then_body
                    .iter()
                    .any(|statement| statement_mentions_name(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_mentions_name(statement, name))
        }
        Statement::Return(value) => value.as_ref().is_some_and(expression_mentions),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_mentions(scrutinee)
                || arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(value) => expression_mentions(value),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_mentions_name(statement, name)),
                })
                || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(value) => expression_mentions(value),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_mentions_name(statement, name)),
                })
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer.as_ref().is_some_and(expression_mentions)
                || condition.as_ref().is_some_and(expression_mentions)
                || step.as_ref().is_some_and(expression_mentions)
                || body
                    .iter()
                    .any(|statement| statement_mentions_name(statement, name))
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
    }
}

fn void_expression_statement(statement: &Statement) -> bool {
    match statement {
        Statement::Store { .. } | Statement::Assign { .. } => true,
        Statement::Expression(expression) => assignment_sequence(expression),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().all(void_expression_statement)
                && else_body.iter().all(void_expression_statement)
        }
        _ => false,
    }
}

fn assignment_sequence(expression: &Expression) -> bool {
    match expression {
        Expression::Assign { .. } => true,
        Expression::Comma { left, right } => {
            assignment_sequence(left) && assignment_sequence(right)
        }
        _ => false,
    }
}

fn statement_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Expression(Expression::Cast {
                target_type: Type::Void,
                operand,
            }) if matches!(operand.as_ref(), Expression::IntegerLiteral(_)) => 0,
            Statement::If {
                then_body,
                else_body,
                ..
            } => 1 + statement_count(then_body) + statement_count(else_body),
            _ => 1,
        })
        .sum()
}

fn inert_integer_void_statement(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if matches!(operand.as_ref(), Expression::IntegerLiteral(_))
    )
}

fn statement_expression(statement: &Statement) -> Option<Expression> {
    match statement {
        Statement::Expression(expression) => Some(expression.clone()),
        Statement::Assign { name, value } => Some(Expression::Assign {
            target: Box::new(Expression::Variable(name.clone())),
            value: Box::new(value.clone()),
        }),
        Statement::Store { target, value } => Some(Expression::Assign {
            target: Box::new(target.clone()),
            value: Box::new(value.clone()),
        }),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            if let Some(value) = crate::analysis::constant_value(condition) {
                return statement_sequence(if value != 0 {
                    then_body
                } else {
                    else_body
                });
            }
            Some(Expression::Conditional {
                condition: Box::new(condition.clone()),
                when_true: Box::new(statement_sequence(then_body)?),
                when_false: Box::new(statement_sequence(else_body)?),
                origin: ConditionalOrigin::IfAssignments,
            })
        }
        Statement::InlineAsm(_)
        | Statement::Return(_)
        | Statement::Switch { .. }
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => None,
    }
}

fn statement_sequence(statements: &[Statement]) -> Option<Expression> {
    let mut expressions = statements
        .iter()
        .map(statement_expression)
        .collect::<Option<Vec<_>>>()?;
    expressions.push(Expression::IntegerLiteral(0));
    Some(sequence(expressions))
}

fn sequence(expressions: Vec<Expression>) -> Expression {
    expressions
        .into_iter()
        .rev()
        .reduce(|right, left| Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        })
        .expect("a value-inline sequence always contains its return expression")
}

/// Ordinary definitions are eligible for automatic value inlining only when
/// they are a direct expression body. More involved selection summaries remain
/// limited to definitions the frontend identified as explicitly/skipped inline.
pub(super) fn summarize_automatic(function: &Function) -> Option<ValueInlineBody> {
    if !function.locals.is_empty() {
        return None;
    }
    if let Some(expression) = summarize_guarded_early_return(function) {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression,
            automatic_transaction: false,
        });
    }
    if !function.statements.is_empty() {
        return None;
    }
    summarize(function)
}

/// Summarize a bounded scalar transaction selected by the automatic inliner.
///
/// SDK-facing wrappers commonly publish a few fields, call a shared queue
/// helper into a scalar local, and return that local. The ordinary direct-value
/// gate above intentionally rejects locals, but the value composer already
/// alpha-renames them and preserves each effect in a comma sequence. Keep this
/// eligibility class narrow: no local storage with identity, no non-local
/// control flow, and a small statement budget.
pub(super) fn summarize_automatic_transaction(function: &Function) -> Option<ValueInlineBody> {
    if function.return_type == Type::Void
        || function.locals.is_empty()
        || function.locals.len() > 4
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || statement_count(&function.statements) > 9
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || local.array_length.is_some()
                || matches!(local.declared_type, Type::Void | Type::Struct { .. })
        })
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name))
                if function.locals.iter().any(|local| local.name == *name)
        )
        || !function
            .statements
            .iter()
            .any(crate::analysis::statement_has_call)
    {
        return None;
    }
    summarize(function).map(|mut body| {
        if extended_diagnostic_transaction(function) {
            body.expression = summarize_sequenced_body_with_policy(
                function,
                normalize_reference_result(
                    function.return_type,
                    function
                        .return_expression
                        .clone()
                        .expect("transaction eligibility checked its return expression"),
                ),
                true,
            )
            .expect("a summarized transaction remains a sequenced body");
        }
        body.automatic_transaction = true;
        body
    })
}

fn extended_diagnostic_transaction(function: &Function) -> bool {
    statement_count(&function.statements) > 8
        && statements_contain_compile_time_branch(&function.statements)
}

fn statements_contain_compile_time_branch(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::constant_value(condition).is_some()
                || statements_contain_compile_time_branch(then_body)
                || statements_contain_compile_time_branch(else_body)
        }
        Statement::Loop { body, .. } => statements_contain_compile_time_branch(body),
        Statement::Switch {
            arms, default, ..
        } => {
            arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statements_contain_compile_time_branch(body)
                }
            }) || default.as_ref().is_some_and(|body| match body {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => {
                    statements_contain_compile_time_branch(body)
                }
            })
        }
        _ => false,
    })
}

/// Summarize a repeated guarded transaction whose true edge owns effects and
/// returns one while the fallthrough returns zero.
///
/// SDK state machines use this shape for cancellation/finalization helpers:
/// publish state, invoke guarded callbacks, then report whether the caller
/// should return. The value composer can alpha-rename the helper's scalar
/// locals and `compose_guarded_truth_value` restores the source-level branch
/// at each caller, so this remains semantic composition rather than a
/// target-specific instruction capture.
pub(super) fn summarize_automatic_guarded_transaction(
    function: &Function,
) -> Option<ValueInlineBody> {
    if !function.is_static
        || matches!(function.return_type, Type::Void | Type::Struct { .. })
        || function.locals.len() > 4
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || local.array_length.is_some()
                || matches!(local.declared_type, Type::Void | Type::Struct { .. })
        })
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let fallback = function.return_expression.as_ref()?;
    if !else_body.is_empty()
        || !matches!(fallback, Expression::IntegerLiteral(0))
        || !matches!(then_body.last(), Some(Statement::Return(Some(Expression::IntegerLiteral(1)))))
        || statement_count(&function.statements) > 16
        || crate::analysis::expression_has_side_effect(condition)
    {
        return None;
    }
    let mut calls = std::collections::HashMap::new();
    super::collect_function_calls(function, &mut calls);
    if calls.values().sum::<usize>() < 2 {
        return None;
    }
    let expression = Expression::Conditional {
        condition: Box::new(condition.clone()),
        when_true: Box::new(summarize_return_arm(then_body, function.return_type)?),
        when_false: Box::new(normalize_reference_result(
            function.return_type,
            fallback.clone(),
        )),
        origin: ConditionalOrigin::IfReturns,
    };
    let first_effect = statement_expression(then_body.first()?)?;
    if function.parameters.iter().any(|parameter| {
        let total = super::safety::expression_use_count(&expression, &parameter.name);
        total > 1
            || (total == 1
                && super::safety::expression_use_count(&first_effect, &parameter.name) != 1)
    }) {
        return None;
    }
    Some(ValueInlineBody {
        source: function.clone(),
        expression,
        automatic_transaction: true,
    })
}

/// Summarize a same-TU scalar helper whose only control flow is a chain of
/// positive guards ending in an early value return:
///
/// `if (a) if (b) return x; return y;` becomes `a && b ? x : y`.
///
/// This retains short-circuit order and gives the ordinary automatic inliner a
/// semantic value body without teaching it arbitrary statement splicing.
fn summarize_guarded_early_return(function: &Function) -> Option<Expression> {
    if !function.guards.is_empty()
        || function.asm_body.is_some()
        || function.return_type == Type::Void
    {
        return None;
    }
    let fallback = function.return_expression.clone()?;
    let mut statements = function.statements.as_slice();
    let mut conditions = Vec::new();
    let early = loop {
        match statements {
            [Statement::If {
                condition,
                then_body,
                else_body,
            }] if else_body.is_empty() => {
                conditions.push(condition.clone());
                statements = then_body;
            }
            [Statement::Return(Some(value))] => break value.clone(),
            _ => return None,
        }
    };
    let condition = conditions
        .into_iter()
        .reduce(|left, right| Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(left),
            right: Box::new(right),
        })?;
    Some(Expression::Conditional {
        condition: Box::new(condition),
        when_true: Box::new(early),
        when_false: Box::new(fallback),
        origin: ConditionalOrigin::IfReturns,
    })
}

/// Ordinary one-use void wrappers are also automatic-inline candidates when
/// their entire body is one expression. Unlike statement-body composition,
/// the value representation can materialize changing caller arguments once at
/// the call site before substituting the wrapper, so branch-assigned values do
/// not prevent a semantics-preserving expansion.
pub(super) fn summarize_automatic_void_forward(function: &Function) -> Option<ValueInlineBody> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || function.return_expression.is_some()
        || !matches!(function.statements.as_slice(), [Statement::Expression(_)])
    {
        return None;
    }
    let [Statement::Expression(expression)] = function.statements.as_slice() else {
        unreachable!("single expression was checked")
    };
    Some(ValueInlineBody {
        source: function.clone(),
        expression: Expression::Comma {
            left: Box::new(expression.clone()),
            right: Box::new(Expression::IntegerLiteral(0)),
        },
        automatic_transaction: false,
    })
}

fn is_boolean_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { operator, .. } => matches!(
            operator,
            BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual
                | BinaryOperator::LogicalAnd
                | BinaryOperator::LogicalOr
        ),
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            ..
        } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{
        BinaryOperator, GuardedReturn, LocalDeclaration, Parameter, Pointee,
    };

    fn empty_function(name: &str, return_type: Type) -> Function {
        Function {
            return_type,
            name: name.into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
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
    fn summarizes_a_direct_member_accessor_for_automatic_inlining() {
        let mut function = empty_function("get", Type::Pointer(mwcc_syntax_trees::Pointee::Int));
        function.parameters.push(Parameter {
            parameter_type: Type::StructPointer { element_size: 16 },
            name: "object".into(),
        });
        function.return_expression = Some(Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 4,
            member_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
            index_stride: None,
        });

        let summary = summarize_automatic(&function).expect("direct accessor");
        assert!(matches!(
            summary.expression,
            Expression::Member { offset: 4, .. }
        ));
    }

    #[test]
    fn summarizes_a_bounded_automatic_value_transaction() {
        let mut function = empty_function("start_async", Type::Int);
        function.parameters.push(Parameter {
            parameter_type: Type::StructPointer { element_size: 48 },
            name: "block".into(),
        });
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "result".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        function.statements = vec![
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("block".into())),
                    offset: 8,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(7),
            },
            Statement::Assign {
                name: "result".into(),
                value: Expression::Call {
                    name: "issue".into(),
                    arguments: vec![Expression::Variable("block".into())],
                },
            },
        ];
        function.return_expression = Some(Expression::Variable("result".into()));

        let summary =
            summarize_automatic_transaction(&function).expect("bounded value transaction");
        assert!(summary.automatic_transaction);
        assert!(matches!(summary.expression, Expression::Comma { .. }));
    }

    #[test]
    fn forwards_a_diagnostic_transactions_terminal_call_result() {
        let mut function = empty_function("start_checked_async", Type::Int);
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "idle".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        function.statements = (0..6)
            .map(|index| Statement::Store {
                target: Expression::Variable(format!("published_{index}")),
                value: Expression::IntegerLiteral(index),
            })
            .chain([
                Statement::If {
                    condition: Expression::IntegerLiteral(1),
                    then_body: vec![Statement::Expression(Expression::Call {
                        name: "diagnose".into(),
                        arguments: Vec::new(),
                    })],
                    else_body: Vec::new(),
                },
                Statement::Assign {
                    name: "idle".into(),
                    value: Expression::Call {
                        name: "issue".into(),
                        arguments: Vec::new(),
                    },
                },
            ])
            .collect();
        function.return_expression = Some(Expression::Variable("idle".into()));

        assert_eq!(statement_count(&function.statements), 9);
        let summary = summarize_automatic_transaction(&function)
            .expect("a bounded diagnostic transaction should be retained");
        assert!(summary.forwards_known_function_designators());
        assert_eq!(
            super::super::safety::expression_use_count(&summary.expression, "idle"),
            0
        );
        let mut tail = &summary.expression;
        while let Expression::Comma { right, .. } = tail {
            tail = right;
        }
        assert!(matches!(
            tail,
            Expression::Call { name, .. } if name == "issue"
        ));
    }

    #[test]
    fn summarizes_a_repeated_guarded_effect_transaction() {
        let mut function = empty_function("finish_if_requested", Type::Int);
        function.is_static = true;
        function.locals.push(LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 48 },
            name: "finished".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        function.statements = vec![Statement::If {
            condition: Expression::Variable("requested".into()),
            then_body: vec![
                Statement::Assign {
                    name: "finished".into(),
                    value: Expression::Variable("executing".into()),
                },
                Statement::If {
                    condition: Expression::Variable("callback".into()),
                    then_body: vec![Statement::Expression(Expression::Call {
                        name: "notify".into(),
                        arguments: vec![Expression::Variable("finished".into())],
                    })],
                    else_body: Vec::new(),
                },
                Statement::Expression(Expression::Call {
                    name: "ready".into(),
                    arguments: Vec::new(),
                }),
                Statement::Return(Some(Expression::IntegerLiteral(1))),
            ],
            else_body: Vec::new(),
        }];
        function.return_expression = Some(Expression::IntegerLiteral(0));

        let summary = summarize_automatic_guarded_transaction(&function)
            .expect("the guarded callback transaction should be repeatable");

        assert!(summary.automatic_transaction);
        assert!(matches!(
            summary.expression,
            Expression::Conditional {
                when_false,
                ..
            } if matches!(when_false.as_ref(), Expression::IntegerLiteral(0))
        ));
    }

    #[test]
    fn ignores_compiled_out_assertion_remnants_in_the_transaction_budget() {
        let mut function = empty_function("start_async", Type::Int);
        function.parameters.push(Parameter {
            parameter_type: Type::StructPointer { element_size: 48 },
            name: "block".into(),
        });
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "result".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        let inert = Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand: Box::new(Expression::IntegerLiteral(0)),
        });
        function.statements = vec![
            inert.clone(),
            inert.clone(),
            inert.clone(),
            inert.clone(),
            inert.clone(),
            inert.clone(),
            inert,
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("block".into())),
                    offset: 8,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(14),
            },
            Statement::Assign {
                name: "result".into(),
                value: Expression::Call {
                    name: "issue".into(),
                    arguments: vec![Expression::Variable("block".into())],
                },
            },
        ];
        function.return_expression = Some(Expression::Variable("result".into()));

        assert_eq!(statement_count(&function.statements), 2);
        assert!(summarize_automatic_transaction(&function).is_some());
    }

    #[test]
    fn preserves_a_reference_accessor_as_an_address_valued_summary() {
        let mut function = empty_function(
            "get_reference",
            Type::StructPointer { element_size: 20 },
        );
        function.parameters.push(Parameter {
            parameter_type: Type::StructPointer { element_size: 56 },
            name: "object".into(),
        });
        function.return_expression = Some(Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 16,
            member_type: Type::Struct { size: 20, align: 4 },
            index_stride: None,
        });

        let summary = summarize_automatic(&function).expect("reference accessor");
        assert!(matches!(
            summary.expression,
            Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Member { offset: 16, .. })
        ));
    }

    #[test]
    fn recognizes_single_use_parameters_in_an_interleaved_forwarding_call() {
        let mut function = empty_function("forward", Type::Void);
        function.parameters = vec![
            Parameter {
                parameter_type: Type::StructPointer { element_size: 16 },
                name: "object".into(),
            },
            Parameter {
                parameter_type: Type::Float,
                name: "value".into(),
            },
        ];
        function.statements = vec![Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![
                Expression::Variable("object".into()),
                Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 4,
                    member_type: Type::Float,
                    index_stride: None,
                },
                Expression::Variable("value".into()),
            ],
        })];

        let summary = summarize_automatic_void_forward(&function).expect("void forwarder");
        assert!(!summary.arguments_forwarded_once_in_order());
        assert!(summary.parameter_used_once_in_forwarded_call("value"));
        assert!(!summary.parameter_used_once_in_forwarded_call("object"));
    }

    #[test]
    fn summarizes_an_asserted_integer_selection() {
        let function = Function {
            return_type: Type::Int,
            name: "selected".into(),
            is_static: true,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "input".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "result".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Expression(Expression::Variable("assertion".into())),
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::If {
                    condition: Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(Expression::Variable("input".into())),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                    then_body: vec![Statement::Assign {
                        name: "result".into(),
                        value: Expression::IntegerLiteral(1),
                    }],
                    else_body: Vec::new(),
                },
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let summary = summarize(&function).expect("selection body should summarize");
        assert!(matches!(summary.expression, Expression::Comma { right, .. }
        if matches!(right.as_ref(), Expression::Binary {
            operator: BinaryOperator::NotEqual,
            ..
        })));
    }

    #[test]
    fn summarizes_a_nested_guarded_early_return_for_automatic_inlining() {
        let mut function = empty_function("translate", Type::UnsignedInt);
        function.parameters.push(Parameter {
            parameter_type: Type::UnsignedInt,
            name: "address".into(),
        });
        function.statements = vec![Statement::If {
            condition: Expression::Variable("in_range".into()),
            then_body: vec![Statement::If {
                condition: Expression::Variable("enabled".into()),
                then_body: vec![Statement::Return(Some(Expression::Variable(
                    "address".into(),
                )))],
                else_body: Vec::new(),
            }],
            else_body: Vec::new(),
        }];
        function.return_expression = Some(Expression::IntegerLiteral(0));

        let summary = summarize_automatic(&function).expect("guarded value summary");
        assert!(matches!(
            summary.expression,
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } if matches!(condition.as_ref(), Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                ..
            })
                && matches!(when_true.as_ref(), Expression::Variable(name) if name == "address")
                && matches!(when_false.as_ref(), Expression::IntegerLiteral(0))
        ));
    }

    #[test]
    fn summarizes_a_guarded_store_before_a_value_return() {
        let mut function = empty_function("take_flag", Type::Int);
        function.parameters.push(Parameter {
            parameter_type: Type::Pointer(Pointee::Int),
            name: "flag".into(),
        });
        let flag = Expression::Dereference {
            pointer: Box::new(Expression::Variable("flag".into())),
        };
        function.statements = vec![Statement::If {
            condition: flag.clone(),
            then_body: vec![
                Statement::Store {
                    target: flag,
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Return(Some(Expression::IntegerLiteral(1))),
            ],
            else_body: vec![Statement::Return(Some(Expression::IntegerLiteral(0)))],
        }];

        let summary = summarize(&function).expect("guarded store return should summarize");
        assert!(matches!(
            summary.expression,
            Expression::Conditional {
                when_true,
                when_false,
                origin: ConditionalOrigin::IfReturns,
                ..
            } if matches!(when_true.as_ref(), Expression::Comma {
                left,
                right,
            } if matches!(left.as_ref(), Expression::Assign { .. })
                && matches!(right.as_ref(), Expression::IntegerLiteral(1)))
                && matches!(when_false.as_ref(), Expression::IntegerLiteral(0))
        ));
    }

    #[test]
    fn summarizes_retained_callback_or_fallback_guards() {
        let mut function = empty_function("dispatch", Type::Int);
        function.parameters.push(Parameter {
            parameter_type: Type::Pointer(Pointee::Int),
            name: "value".into(),
        });
        function.guards.push(GuardedReturn {
            condition: Expression::Variable("callback".into()),
            value: Expression::CallThrough {
                target: Box::new(Expression::Variable("callback".into())),
                arguments: vec![Expression::Variable("value".into())],
            },
        });
        function.return_expression = Some(Expression::Call {
            name: "fallback".into(),
            arguments: vec![Expression::Variable("value".into())],
        });

        let summary = summarize(&function).expect("guard chain should summarize");
        assert!(matches!(
            summary.expression,
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                origin: ConditionalOrigin::IfReturns,
            } if matches!(condition.as_ref(), Expression::Variable(name) if name == "callback")
                && matches!(when_true.as_ref(), Expression::CallThrough { .. })
                && matches!(when_false.as_ref(), Expression::Call { name, .. } if name == "fallback")
        ));
    }
}
