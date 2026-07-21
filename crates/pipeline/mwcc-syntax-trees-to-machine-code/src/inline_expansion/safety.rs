//! Conservative eligibility and alias-safety checks for AST inline expansion.

use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::HashSet;

pub(super) fn composable_function(function: &Function) -> bool {
    function.return_type == Type::Void
        && function.locals.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
        && composable_statements(&function.statements)
        && function
            .parameters
            .iter()
            .all(|parameter| !variable_is_modified_or_escaped(function, &parameter.name))
}

fn composable_statements(statements: &[Statement]) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. } | Statement::Expression(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => composable_statements(then_body) && composable_statements(else_body),
        Statement::Assign { .. }
        | Statement::Return(_)
        | Statement::Switch { .. }
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => false,
    })
}

fn stable_argument(expression: &Expression, stable_variables: &HashSet<String>) -> bool {
    match expression {
        Expression::Variable(name) => stable_variables.contains(name),
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
        _ => false,
    }
}

/// Whether substituting call arguments into this retained body preserves
/// evaluation count. Stable scalar values are always safe. One otherwise
/// impure argument is also safe when a one-store setter consumes it exactly
/// once as the stored value: substitution neither duplicates nor drops the
/// evaluation and there is no earlier callee-body effect to reorder it with.
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
    let [unstable_index] = unstable.as_slice() else {
        return false;
    };
    let [Statement::Store { target, value }] = function.statements.as_slice() else {
        return false;
    };
    let parameter = &function.parameters[*unstable_index].name;
    !expression_mentions(target, parameter) && expression_use_count(value, parameter) == 1
}

fn expression_use_count(expression: &Expression, name: &str) -> usize {
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

fn statement_modifies_or_escapes(statement: &Statement, name: &str) -> bool {
    match statement {
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
        Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => expression_mentions(operand, name),
        Expression::Assign { target, value } => {
            expression_mentions(target, name) || expression_modifies_or_escapes(value, name)
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
