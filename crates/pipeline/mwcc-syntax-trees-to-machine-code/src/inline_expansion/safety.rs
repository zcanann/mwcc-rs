//! Conservative eligibility and alias-safety checks for AST inline expansion.

use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::HashSet;

pub(super) fn composable_function(function: &Function) -> bool {
    let local_names: HashSet<&str> = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect();
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
                && local.array_length.is_none()
                && local.initializer.is_some()
        })
        && function.guards.is_empty()
        && (function.return_expression.is_none()
            || matches!(function.return_expression, Some(Expression::Variable(ref name)) if name == "this"))
        && function.asm_body.is_none()
        && composable_statements(&function.statements, &local_names)
        && function
            .parameters
            .iter()
            .all(|parameter| !variable_is_modified_or_escaped(function, &parameter.name))
}

fn composable_statements(statements: &[Statement], local_names: &HashSet<&str>) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. } | Statement::Expression(_) => true,
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            composable_statements(then_body, local_names)
                && composable_statements(else_body, local_names)
        }
        // A void return is local control flow, not an escape from the caller.
        // Expansion rewrites it to a forward jump to the end of this particular
        // inline instance before the body enters instruction selection.
        Statement::Return(None) => true,
        Statement::Return(Some(_))
        | Statement::Switch { .. }
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => false,
    })
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

fn stable_lvalue_address(expression: &Expression, stable_variables: &HashSet<String>) -> bool {
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
) -> bool {
    function.parameters.len() == arguments.len()
        && arguments.iter().all(|argument| {
            stable_argument(argument, stable_variables)
                || matches!(
                    argument,
                    Expression::Member {
                        base,
                        member_type,
                        index_stride: None,
                        ..
                    } if !matches!(member_type, Type::Void | Type::Struct { .. })
                        && stable_argument(base, stable_variables)
                )
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
        // `&local` exposes the local object's storage. `&pointer->member` only
        // exposes the pointee; it cannot change the pointer value substituted
        // into a retained inline body.
        Expression::AddressOf { operand } => {
            matches!(operand.as_ref(), Expression::Variable(variable) if variable == name)
        }
        Expression::PostStep {
            target: operand, ..
        } => expression_mentions(operand, name),
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
