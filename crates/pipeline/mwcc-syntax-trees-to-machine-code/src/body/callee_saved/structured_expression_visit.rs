//! Shared source-tree traversal for structured-body planning.
//!
//! Plans use this read-only walk to discover value relationships before any
//! instruction is emitted. Keeping the traversal here prevents each plan from
//! growing its own subtly different list of expression and statement forms.

use mwcc_syntax_trees::{ArmBody, Expression, Statement};

pub(super) fn visit_statement(statement: &Statement, visit: &mut impl FnMut(&Expression)) {
    match statement {
        Statement::Store { target, value } => {
            visit_expression(target, visit);
            visit_expression(value, visit);
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            visit_expression(value, visit);
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            visit_expression(condition, visit);
            for statement in then_body.iter().chain(else_body) {
                visit_statement(statement, visit);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                visit_expression(value, visit);
            }
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            visit_expression(scrutinee, visit);
            for arm in arms {
                visit_arm_body(&arm.body, visit);
            }
            if let Some(default) = default {
                visit_arm_body(default, visit);
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
                visit_expression(expression, visit);
            }
            for statement in body {
                visit_statement(statement, visit);
            }
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => {}
    }
}

pub(super) fn visit_expression(expression: &Expression, visit: &mut impl FnMut(&Expression)) {
    visit(expression);
    match expression {
        Expression::AggregateLiteral(elements) => {
            for element in elements {
                visit_expression(element, visit);
            }
        }
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            visit_expression(left, visit);
            visit_expression(right, visit);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            visit_expression(condition, visit);
            visit_expression(when_true, visit);
            visit_expression(when_false, visit);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => visit_expression(operand, visit),
        Expression::BitFieldRead {
            extracted, storage, ..
        }
        | Expression::Index {
            base: extracted,
            index: storage,
        } => {
            visit_expression(extracted, visit);
            visit_expression(storage, visit);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            visit_expression(base, visit);
        }
        Expression::CallThrough { target, arguments } => {
            visit_expression(target, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            visit_expression(object, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            visit_expression(allocation, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

fn visit_arm_body(body: &ArmBody, visit: &mut impl FnMut(&Expression)) {
    match body {
        ArmBody::Return(expression) => visit_expression(expression, visit),
        ArmBody::Statements(statements) => {
            for statement in statements {
                visit_statement(statement, visit);
            }
        }
    }
}
