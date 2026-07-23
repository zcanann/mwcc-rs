//! Translation-unit call-site counting for automatic inline candidates.

use mwcc_syntax_trees::{Expression, Function, Statement};
use std::collections::HashMap;

pub(super) fn collect_function_calls(function: &Function, counts: &mut HashMap<String, usize>) {
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            collect_expression_calls(initializer, counts);
        }
    }
    for guard in &function.guards {
        collect_expression_calls(&guard.condition, counts);
        collect_expression_calls(&guard.value, counts);
    }
    if let Some(value) = &function.return_expression {
        collect_expression_calls(value, counts);
    }
    collect_statement_calls(&function.statements, counts);
}

fn collect_statement_calls(statements: &[Statement], counts: &mut HashMap<String, usize>) {
    for statement in statements {
        match statement {
            Statement::Store { target, value } => {
                collect_expression_calls(target, counts);
                collect_expression_calls(value, counts);
            }
            Statement::Assign { value, .. } | Statement::Expression(value) => {
                collect_expression_calls(value, counts);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_expression_calls(condition, counts);
                collect_statement_calls(then_body, counts);
                collect_statement_calls(else_body, counts);
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    collect_expression_calls(value, counts);
                }
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                collect_expression_calls(scrutinee, counts);
                for arm in arms {
                    collect_arm_calls(&arm.body, counts);
                }
                if let Some(default) = default {
                    collect_arm_calls(default, counts);
                }
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    collect_expression_calls(expression, counts);
                }
                collect_statement_calls(body, counts);
            }
            Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {}
        }
    }
}

fn collect_arm_calls(arm: &mwcc_syntax_trees::ArmBody, counts: &mut HashMap<String, usize>) {
    match arm {
        mwcc_syntax_trees::ArmBody::Return(value) => collect_expression_calls(value, counts),
        mwcc_syntax_trees::ArmBody::Statements(body) => collect_statement_calls(body, counts),
    }
}

fn collect_expression_calls(expression: &Expression, counts: &mut HashMap<String, usize>) {
    match expression {
        Expression::Call { name, arguments } => {
            *counts.entry(name.clone()).or_default() += 1;
            for argument in arguments {
                collect_expression_calls(argument, counts);
            }
        }
        Expression::AggregateLiteral(elements) => {
            for element in elements {
                collect_expression_calls(element, counts);
            }
        }
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            collect_expression_calls(left, counts);
            collect_expression_calls(right, counts);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_expression_calls(condition, counts);
            collect_expression_calls(when_true, counts);
            collect_expression_calls(when_false, counts);
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
        }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => {
            collect_expression_calls(operand, counts)
        }
        Expression::Index { base, index } => {
            collect_expression_calls(base, counts);
            collect_expression_calls(index, counts);
        }
        Expression::CallThrough { target, arguments } => {
            collect_expression_calls(target, counts);
            for argument in arguments {
                collect_expression_calls(argument, counts);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_expression_calls(object, counts);
            for argument in arguments {
                collect_expression_calls(argument, counts);
            }
        }
        // Allocation and construction are one guarded ABI expression, not an
        // ordinary source call site that this source-body expander may replace.
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            collect_expression_calls(allocation, counts);
            for argument in arguments {
                collect_expression_calls(argument, counts);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}
