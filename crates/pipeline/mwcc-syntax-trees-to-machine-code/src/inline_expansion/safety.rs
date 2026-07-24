//! Conservative eligibility and alias-safety checks for AST inline expansion.

use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::HashSet;

pub(super) fn composable_function(function: &Function) -> bool {
    composable_function_with_assignable_parameters(function, false)
        && function
            .parameters
            .iter()
            .all(|parameter| !variable_is_modified_or_escaped(function, &parameter.name))
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
                && local.array_length.is_none()
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

/// Apply MWCC's small-body gate to ordinary one-call definitions newly made
/// composable by dominated, uninitialized locals. Explicit inline definitions
/// retain the broader semantic safety check above. Previously composable
/// initialized-local bodies also retain their established behavior.
pub(super) fn automatic_composable_function(function: &Function) -> bool {
    let ordinary = composable_function(function)
        && (function
            .locals
            .iter()
            .all(|local| local.initializer.is_some())
            || statement_weight(&function.statements) <= 4);
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

fn statement_weight(statements: &[Statement]) -> usize {
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
        .filter(|local| local.initializer.is_none())
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
            Statement::Switch { .. }
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_)
            | Statement::Loop { .. } => return false,
        }
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
    allow_changing_scalars: bool,
) -> bool {
    function.parameters.len() == arguments.len()
        && function
            .parameters
            .iter()
            .zip(arguments)
            .all(|(parameter, argument)| {
                stable_argument(argument, stable_variables)
                    // A scalar local read is side-effect-free at the call site.
                    // Copying it into a hygienic inline parameter preserves the
                    // ordinary once-only argument evaluation even when that
                    // caller local is reassigned elsewhere in the function.
                    || (automatic_parameter_select_store_body(function)
                        && matches!(argument, Expression::Variable(_)))
                    || (allow_changing_scalars
                        && matches!(argument, Expression::Variable(_))
                        && !matches!(parameter.parameter_type, Type::Void | Type::Struct { .. }))
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
