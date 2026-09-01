//! Shared source-tree traversal for structured-body planning.
//!
//! Plans use these visitors and clone/rewrite helpers to discover and expose
//! value relationships before any instruction is emitted. Keeping traversal
//! here prevents each plan from growing its own subtly different list of
//! expression and statement forms.

use mwcc_syntax_trees::{ArmBody, Expression, Statement};

pub(in crate::body) fn visit_statement(
    statement: &Statement,
    visit: &mut impl FnMut(&Expression),
) {
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

pub(in crate::body) fn visit_expression(
    expression: &Expression,
    visit: &mut impl FnMut(&Expression),
) {
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

/// Whether a statement tree can replace the value held by `name`.
///
/// This complements the expression visitor: statement assignments carry their
/// target outside the expression tree, while C assignment expressions and
/// post-step operators remain owned by the common expression analysis.
pub(super) fn statement_assigns_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Expression(expression) => {
            crate::analysis::expression_assigns_name(expression, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::expression_assigns_name(condition, name)
                || statements_assign_name(then_body, name)
                || statements_assign_name(else_body, name)
        }
        Statement::Return(value) => value
            .as_ref()
            .is_some_and(|value| crate::analysis::expression_assigns_name(value, name)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            crate::analysis::expression_assigns_name(scrutinee, name)
                || arms.iter().any(|arm| arm_body_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_assigns_name(body, name))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            [initializer, condition, step]
                .into_iter()
                .flatten()
                .any(|expression| crate::analysis::expression_assigns_name(expression, name))
                || statements_assign_name(body, name)
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

pub(in crate::body) fn statements_assign_name(statements: &[Statement], name: &str) -> bool {
    statements
        .iter()
        .any(|statement| statement_assigns_name(statement, name))
}

fn arm_body_assigns_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(expression) => crate::analysis::expression_assigns_name(expression, name),
        ArmBody::Statements(statements) => statements_assign_name(statements, name),
    }
}

/// Clone a statement tree while allowing a planner to replace complete
/// expression nodes. Replacements are pre-order and terminal: once the
/// callback returns a node, its children are not rewritten a second time.
pub(in crate::body) fn rewrite_statement(
    statement: &Statement,
    rewrite: &mut impl FnMut(&Expression) -> Option<Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: rewrite_expression(target, rewrite),
            value: rewrite_expression(value, rewrite),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: rewrite_expression(value, rewrite),
        },
        Statement::Expression(expression) => {
            Statement::Expression(rewrite_expression(expression, rewrite))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: rewrite_expression(condition, rewrite),
            then_body: then_body
                .iter()
                .map(|statement| rewrite_statement(statement, rewrite))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| rewrite_statement(statement, rewrite))
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|value| rewrite_expression(value, rewrite)),
        ),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => Statement::Switch {
            scrutinee: rewrite_expression(scrutinee, rewrite),
            arms: arms
                .iter()
                .map(|arm| mwcc_syntax_trees::SwitchArm {
                    value: arm.value,
                    body: rewrite_arm_body(&arm.body, rewrite),
                    falls_through: arm.falls_through,
                })
                .collect(),
            default: default
                .as_ref()
                .map(|body| rewrite_arm_body(body, rewrite)),
        },
        Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } => Statement::Loop {
            kind: *kind,
            initializer: initializer
                .as_ref()
                .map(|value| rewrite_expression(value, rewrite)),
            condition: condition
                .as_ref()
                .map(|value| rewrite_expression(value, rewrite)),
            step: step
                .as_ref()
                .map(|value| rewrite_expression(value, rewrite)),
            body: body
                .iter()
                .map(|statement| rewrite_statement(statement, rewrite))
                .collect(),
        },
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => statement.clone(),
    }
}

pub(super) fn rewrite_expression(
    expression: &Expression,
    rewrite: &mut impl FnMut(&Expression) -> Option<Expression>,
) -> Expression {
    if let Some(replacement) = rewrite(expression) {
        return replacement;
    }
    match expression {
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| rewrite_expression(element, rewrite))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(rewrite_expression(left, rewrite)),
            right: Box::new(rewrite_expression(right, rewrite)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(rewrite_expression(operand, rewrite)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(rewrite_expression(condition, rewrite)),
            when_true: Box::new(rewrite_expression(when_true, rewrite)),
            when_false: Box::new(rewrite_expression(when_false, rewrite)),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(rewrite_expression(operand, rewrite)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(rewrite_expression(extracted, rewrite)),
            promoted_type: *promoted_type,
            storage: Box::new(rewrite_expression(storage, rewrite)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(rewrite_expression(value, rewrite)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(rewrite_expression(pointer, rewrite)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(rewrite_expression(operand, rewrite)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(rewrite_expression(base, rewrite)),
            index: Box::new(rewrite_expression(index, rewrite)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(rewrite_expression(base, rewrite)),
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
            base: Box::new(rewrite_expression(base, rewrite)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(rewrite_expression(target, rewrite)),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, rewrite))
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
            object: Box::new(rewrite_expression(object, rewrite)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, rewrite))
                .collect(),
        },
        Expression::ConstructedNew {
            allocation,
            allocation_size,
            constructor,
            arguments,
        } => Expression::ConstructedNew {
            allocation: Box::new(rewrite_expression(allocation, rewrite)),
            allocation_size: *allocation_size,
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, rewrite))
                .collect(),
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, rewrite))
                .collect(),
        },
        Expression::PostStep {
            target,
            operator,
            pointer_link,
        } => Expression::PostStep {
            target: Box::new(rewrite_expression(target, rewrite)),
            operator: *operator,
            pointer_link: *pointer_link,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(rewrite_expression(target, rewrite)),
            value: Box::new(rewrite_expression(value, rewrite)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(rewrite_expression(left, rewrite)),
            right: Box::new(rewrite_expression(right, rewrite)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}

fn rewrite_arm_body(
    body: &ArmBody,
    rewrite: &mut impl FnMut(&Expression) -> Option<Expression>,
) -> ArmBody {
    match body {
        ArmBody::Return(expression) => ArmBody::Return(rewrite_expression(expression, rewrite)),
        ArmBody::Statements(statements) => ArmBody::Statements(
            statements
                .iter()
                .map(|statement| rewrite_statement(statement, rewrite))
                .collect(),
        ),
    }
}
