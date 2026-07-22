//! Recursive substitution of expression-valued retained inline calls.

use super::safety::stable_arguments;
use super::substitution::substitute_expression;
use super::value_body::ValueInlineBody;
use mwcc_syntax_trees::{ArmBody, Expression, Statement};
use std::collections::{HashMap, HashSet};

pub(super) fn expand_statement(
    statement: &Statement,
    bodies: &HashMap<String, ValueInlineBody>,
    stable_variables: &HashSet<String>,
    active: &mut HashSet<String>,
    changed: &mut bool,
    value_body_substitutions: &mut usize,
) -> Statement {
    let expression = |value: &Expression,
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
) -> ArmBody {
    match body {
        ArmBody::Return(value) => ArmBody::Return(expand_expression(
            value,
            bodies,
            stable_variables,
            active,
            changed,
            value_body_substitutions,
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
) -> Expression {
    let recurse = |value: &Expression,
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
            if active.contains(name)
                || !stable_arguments(&body.source, &arguments, stable_variables)
            {
                return Expression::Call {
                    name: name.clone(),
                    arguments,
                };
            }
            let replacements: HashMap<_, _> = body
                .source
                .parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .zip(arguments)
                .collect();
            let substituted = substitute_expression(&body.expression, &replacements);
            *changed = true;
            *value_body_substitutions += 1;
            active.insert(name.clone());
            let expanded = recurse(&substituted, active, changed, value_body_substitutions);
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
