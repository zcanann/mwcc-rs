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
) -> Statement {
    let expression = |value: &Expression, active: &mut HashSet<String>, changed: &mut bool| {
        expand_expression(value, bodies, stable_variables, active, changed)
    };
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: expression(target, active, changed),
            value: expression(value, active, changed),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: expression(value, active, changed),
        },
        Statement::Expression(value) => Statement::Expression(expression(value, active, changed)),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: expression(condition, active, changed),
            then_body: then_body
                .iter()
                .map(|statement| {
                    expand_statement(statement, bodies, stable_variables, active, changed)
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| {
                    expand_statement(statement, bodies, stable_variables, active, changed)
                })
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|value| expression(value, active, changed)),
        ),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => Statement::Switch {
            scrutinee: expression(scrutinee, active, changed),
            arms: arms
                .iter()
                .map(|arm| mwcc_syntax_trees::SwitchArm {
                    value: arm.value,
                    body: expand_arm(&arm.body, bodies, stable_variables, active, changed),
                    falls_through: arm.falls_through,
                })
                .collect(),
            default: default
                .as_ref()
                .map(|body| expand_arm(body, bodies, stable_variables, active, changed)),
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
                .map(|value| expression(value, active, changed)),
            condition: condition
                .as_ref()
                .map(|value| expression(value, active, changed)),
            step: step
                .as_ref()
                .map(|value| expression(value, active, changed)),
            body: body
                .iter()
                .map(|statement| {
                    expand_statement(statement, bodies, stable_variables, active, changed)
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
) -> ArmBody {
    match body {
        ArmBody::Return(value) => ArmBody::Return(expand_expression(
            value,
            bodies,
            stable_variables,
            active,
            changed,
        )),
        ArmBody::Statements(statements) => ArmBody::Statements(
            statements
                .iter()
                .map(|statement| {
                    expand_statement(statement, bodies, stable_variables, active, changed)
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
) -> Expression {
    let recurse = |value: &Expression, active: &mut HashSet<String>, changed: &mut bool| {
        expand_expression(value, bodies, stable_variables, active, changed)
    };
    match expression {
        Expression::Call { name, arguments } => {
            let arguments: Vec<_> = arguments
                .iter()
                .map(|argument| recurse(argument, active, changed))
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
            active.insert(name.clone());
            let expanded = recurse(&substituted, active, changed);
            active.remove(name);
            expanded
        }
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| recurse(element, active, changed))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(recurse(left, active, changed)),
            right: Box::new(recurse(right, active, changed)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(recurse(operand, active, changed)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(recurse(condition, active, changed)),
            when_true: Box::new(recurse(when_true, active, changed)),
            when_false: Box::new(recurse(when_false, active, changed)),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(recurse(operand, active, changed)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(recurse(extracted, active, changed)),
            promoted_type: *promoted_type,
            storage: Box::new(recurse(storage, active, changed)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(recurse(value, active, changed)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(recurse(pointer, active, changed)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(recurse(operand, active, changed)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(recurse(base, active, changed)),
            index: Box::new(recurse(index, active, changed)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(recurse(base, active, changed)),
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
            base: Box::new(recurse(base, active, changed)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(recurse(target, active, changed)),
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed))
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
            object: Box::new(recurse(object, active, changed)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed))
                .collect(),
        },
        Expression::PostStep { target, operator } => Expression::PostStep {
            target: Box::new(recurse(target, active, changed)),
            operator: *operator,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(recurse(target, active, changed)),
            value: Box::new(recurse(value, active, changed)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(recurse(left, active, changed)),
            right: Box::new(recurse(right, active, changed)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}
