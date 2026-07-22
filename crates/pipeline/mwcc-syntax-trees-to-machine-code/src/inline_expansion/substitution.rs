//! Hygienic expression substitution for the inline subset.

use mwcc_syntax_trees::{Expression, Statement};
use std::collections::HashMap;

pub(super) fn substitute_statement(
    statement: &Statement,
    replacements: &HashMap<String, Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: substitute_expression(target, replacements),
            value: substitute_expression(value, replacements),
        },
        Statement::Expression(expression) => {
            Statement::Expression(substitute_expression(expression, replacements))
        }
        Statement::Assign { name, value } => Statement::Assign {
            name: replacements
                .get(name)
                .and_then(|replacement| match replacement {
                    Expression::Variable(name) => Some(name.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| name.clone()),
            value: substitute_expression(value, replacements),
        },
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: substitute_expression(condition, replacements),
            then_body: then_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
        },
        _ => statement.clone(),
    }
}

pub(super) fn substitute_expression(
    expression: &Expression,
    replacements: &HashMap<String, Expression>,
) -> Expression {
    match expression {
        Expression::Variable(name) => replacements
            .get(name)
            .map_or_else(|| expression.clone(), Clone::clone),
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| substitute_expression(element, replacements))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute_expression(left, replacements)),
            right: Box::new(substitute_expression(right, replacements)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(substitute_expression(condition, replacements)),
            when_true: Box::new(substitute_expression(when_true, replacements)),
            when_false: Box::new(substitute_expression(when_false, replacements)),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(substitute_expression(extracted, replacements)),
            promoted_type: *promoted_type,
            storage: Box::new(substitute_expression(storage, replacements)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(substitute_expression(value, replacements)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(substitute_expression(pointer, replacements)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(substitute_expression(base, replacements)),
            index: Box::new(substitute_expression(index, replacements)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(substitute_expression(base, replacements)),
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
            base: Box::new(substitute_expression(base, replacements)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(substitute_expression(target, replacements)),
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
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
            object: Box::new(substitute_expression(object, replacements)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
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
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::PostStep { target, operator } => Expression::PostStep {
            target: Box::new(substitute_expression(target, replacements)),
            operator: *operator,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute_expression(target, replacements)),
            value: Box::new(substitute_expression(value, replacements)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(substitute_expression(left, replacements)),
            right: Box::new(substitute_expression(right, replacements)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}
