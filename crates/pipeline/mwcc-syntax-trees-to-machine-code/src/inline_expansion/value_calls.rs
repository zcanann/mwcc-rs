//! Recursive substitution of expression-valued retained inline calls.

use super::safety::{stable_argument, stable_local_values};
use super::substitution::substitute_expression;
use super::value_body::ValueInlineBody;
use mwcc_syntax_trees::{ArmBody, Expression, LocalDeclaration, Statement};
use std::collections::{HashMap, HashSet};

pub(super) struct LocalAllocator<'a> {
    pub(super) locals: &'a mut Vec<LocalDeclaration>,
    pub(super) occupied_names: &'a mut HashSet<String>,
    pub(super) next_local_id: &'a mut usize,
}

pub(super) fn expand_statement(
    statement: &Statement,
    bodies: &HashMap<String, ValueInlineBody>,
    stable_variables: &HashSet<String>,
    active: &mut HashSet<String>,
    changed: &mut bool,
    value_body_substitutions: &mut usize,
    allocator: &mut LocalAllocator<'_>,
) -> Statement {
    let mut expression = |value: &Expression,
                          active: &mut HashSet<String>,
                          changed: &mut bool,
                          value_body_substitutions: &mut usize| {
        expand_expression(
            value,
            bodies,
            stable_variables,
            active,
            changed,
            value_body_substitutions,
            allocator,
        )
    };
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: expression(target, active, changed, value_body_substitutions),
            value: expression(value, active, changed, value_body_substitutions),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: expression(value, active, changed, value_body_substitutions),
        },
        Statement::Expression(value) => {
            Statement::Expression(expression(value, active, changed, value_body_substitutions))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: expression(condition, active, changed, value_body_substitutions),
            then_body: then_body
                .iter()
                .map(|statement| {
                    expand_statement(
                        statement,
                        bodies,
                        stable_variables,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    )
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| {
                    expand_statement(
                        statement,
                        bodies,
                        stable_variables,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    )
                })
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|value| expression(value, active, changed, value_body_substitutions)),
        ),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => Statement::Switch {
            scrutinee: expression(scrutinee, active, changed, value_body_substitutions),
            arms: arms
                .iter()
                .map(|arm| mwcc_syntax_trees::SwitchArm {
                    value: arm.value,
                    body: expand_arm(
                        &arm.body,
                        bodies,
                        stable_variables,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    ),
                    falls_through: arm.falls_through,
                })
                .collect(),
            default: default.as_ref().map(|body| {
                expand_arm(
                    body,
                    bodies,
                    stable_variables,
                    active,
                    changed,
                    value_body_substitutions,
                    allocator,
                )
            }),
        },
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            kind,
        } => Statement::Loop {
            initializer: initializer
                .as_ref()
                .map(|value| expression(value, active, changed, value_body_substitutions)),
            condition: condition
                .as_ref()
                .map(|value| expression(value, active, changed, value_body_substitutions)),
            step: step
                .as_ref()
                .map(|value| expression(value, active, changed, value_body_substitutions)),
            body: body
                .iter()
                .map(|statement| {
                    expand_statement(
                        statement,
                        bodies,
                        stable_variables,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    )
                })
                .collect(),
            kind: *kind,
        },
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
            statement.clone()
        }
    }
}

fn expand_arm(
    body: &ArmBody,
    bodies: &HashMap<String, ValueInlineBody>,
    stable_variables: &HashSet<String>,
    active: &mut HashSet<String>,
    changed: &mut bool,
    value_body_substitutions: &mut usize,
    allocator: &mut LocalAllocator<'_>,
) -> ArmBody {
    match body {
        ArmBody::Return(value) => ArmBody::Return(expand_expression(
            value,
            bodies,
            stable_variables,
            active,
            changed,
            value_body_substitutions,
            allocator,
        )),
        ArmBody::Statements(statements) => ArmBody::Statements(
            statements
                .iter()
                .map(|statement| {
                    expand_statement(
                        statement,
                        bodies,
                        stable_variables,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    )
                })
                .collect(),
        ),
    }
}

pub(super) fn expand_expression(
    expression: &Expression,
    bodies: &HashMap<String, ValueInlineBody>,
    stable_variables: &HashSet<String>,
    active: &mut HashSet<String>,
    changed: &mut bool,
    value_body_substitutions: &mut usize,
    allocator: &mut LocalAllocator<'_>,
) -> Expression {
    let mut recurse = |value: &Expression,
                       active: &mut HashSet<String>,
                       changed: &mut bool,
                       value_body_substitutions: &mut usize| {
        expand_expression(
            value,
            bodies,
            stable_variables,
            active,
            changed,
            value_body_substitutions,
            allocator,
        )
    };
    match expression {
        Expression::Call { name, arguments } => {
            let arguments: Vec<_> = arguments
                .iter()
                .map(|argument| recurse(argument, active, changed, value_body_substitutions))
                .collect();
            let Some(body) = bodies.get(name) else {
                return Expression::Call {
                    name: name.clone(),
                    arguments,
                };
            };
            if active.contains(name) {
                return Expression::Call {
                    name: name.clone(),
                    arguments,
                };
            }
            let mut replacements = HashMap::new();
            let mut argument_initializers = Vec::new();
            for (parameter, argument) in body.source.parameters.iter().zip(arguments) {
                if stable_argument(&argument, stable_variables) {
                    replacements.insert(parameter.name.clone(), argument);
                    continue;
                }
                let unique_name = fresh_name(name, &parameter.name, allocator);
                replacements.insert(
                    parameter.name.clone(),
                    Expression::Variable(unique_name.clone()),
                );
                allocator.locals.push(LocalDeclaration {
                    declared_type: parameter.parameter_type,
                    name: unique_name.clone(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                });
                argument_initializers.push(Expression::Assign {
                    target: Box::new(Expression::Variable(unique_name)),
                    value: Box::new(argument),
                });
            }
            let callee_stable = stable_local_values(&body.source);
            let mut nested_stable_variables = stable_variables.clone();
            for local in &body.source.locals {
                let unique_name = fresh_name(name, &local.name, allocator);
                replacements.insert(
                    local.name.clone(),
                    Expression::Variable(unique_name.clone()),
                );
                if callee_stable.contains(&local.name) {
                    nested_stable_variables.insert(unique_name.clone());
                }
                let mut declaration = local.clone();
                declaration.name = unique_name;
                declaration.initializer = None;
                allocator.locals.push(declaration);
            }
            let substituted = argument_initializers.into_iter().rev().fold(
                substitute_expression(&body.expression, &replacements),
                |right, left| Expression::Comma {
                    left: Box::new(left),
                    right: Box::new(right),
                },
            );
            *changed = true;
            *value_body_substitutions += 1;
            active.insert(name.clone());
            let expanded = expand_expression(
                &substituted,
                bodies,
                &nested_stable_variables,
                active,
                changed,
                value_body_substitutions,
                allocator,
            );
            active.remove(name);
            expanded
        }
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| recurse(element, active, changed, value_body_substitutions))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(recurse(left, active, changed, value_body_substitutions)),
            right: Box::new(recurse(right, active, changed, value_body_substitutions)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(recurse(operand, active, changed, value_body_substitutions)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(recurse(
                condition,
                active,
                changed,
                value_body_substitutions,
            )),
            when_true: Box::new(recurse(
                when_true,
                active,
                changed,
                value_body_substitutions,
            )),
            when_false: Box::new(recurse(
                when_false,
                active,
                changed,
                value_body_substitutions,
            )),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(recurse(operand, active, changed, value_body_substitutions)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(recurse(
                extracted,
                active,
                changed,
                value_body_substitutions,
            )),
            promoted_type: *promoted_type,
            storage: Box::new(recurse(storage, active, changed, value_body_substitutions)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(recurse(value, active, changed, value_body_substitutions)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(recurse(pointer, active, changed, value_body_substitutions)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(recurse(operand, active, changed, value_body_substitutions)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(recurse(base, active, changed, value_body_substitutions)),
            index: Box::new(recurse(index, active, changed, value_body_substitutions)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(recurse(base, active, changed, value_body_substitutions)),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        Expression::MemberAddress {
            base,
            offset,
            element,
            index_stride,
        } => Expression::MemberAddress {
            base: Box::new(recurse(base, active, changed, value_body_substitutions)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(recurse(target, active, changed, value_body_substitutions)),
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed, value_body_substitutions))
                .collect(),
        },
        Expression::VirtualCall {
            object,
            vptr_offset,
            slot_offset,
            return_type,
            variadic,
            arguments,
        } => Expression::VirtualCall {
            object: Box::new(recurse(object, active, changed, value_body_substitutions)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed, value_body_substitutions))
                .collect(),
        },
        Expression::ConstructedNew {
            allocation_size,
            constructor,
            arguments,
        } => Expression::ConstructedNew {
            allocation_size: *allocation_size,
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed, value_body_substitutions))
                .collect(),
        },
        Expression::PostStep { target, operator } => Expression::PostStep {
            target: Box::new(recurse(target, active, changed, value_body_substitutions)),
            operator: *operator,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(recurse(target, active, changed, value_body_substitutions)),
            value: Box::new(recurse(value, active, changed, value_body_substitutions)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(recurse(left, active, changed, value_body_substitutions)),
            right: Box::new(recurse(right, active, changed, value_body_substitutions)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}

fn fresh_name(name: &str, local: &str, allocator: &mut LocalAllocator<'_>) -> String {
    loop {
        let candidate = format!(
            "__mwcc_inline_{}_{}_{}",
            name, *allocator.next_local_id, local
        );
        *allocator.next_local_id += 1;
        if allocator.occupied_names.insert(candidate.clone()) {
            return candidate;
        }
    }
}
