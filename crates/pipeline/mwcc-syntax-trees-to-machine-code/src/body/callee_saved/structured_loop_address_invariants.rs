//! File-scope addresses retained across calls in a structured loop.
//!
//! A direct call argument such as `suspend(&thread)` is loop invariant. When an
//! earlier call in the loop makes the materialized address cross a call edge,
//! optimized MWCC hoists it before the loop and gives it an ordinary saved local
//! home. This normalization exposes that value lifetime to the shared planner;
//! instruction selection remains owned by the normal address and call emitters.

use super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) fn hoist_loop_address_invariants(function: &Function) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let source_locals = used.clone();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let mut changed = false;
    let mut statements = Vec::with_capacity(function.statements.len());

    for statement in &function.statements {
        let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        else {
            statements.push(statement.clone());
            continue;
        };
        let candidates = retained_call_argument_addresses(body, &source_locals);
        if candidates.is_empty() {
            statements.push(statement.clone());
            continue;
        }

        let replacements = candidates
            .into_iter()
            .map(|symbol| {
                let name = fresh_name(&mut used, &mut next_name);
                declarations.push(LocalDeclaration {
                    declared_type: Type::Pointer(Pointee::UnsignedChar),
                    name: name.clone(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    attribute_alignment: None,
                    row_bytes: None,
                });
                statements.push(Statement::Assign {
                    name: name.clone(),
                    value: Expression::AddressOf {
                        operand: Box::new(Expression::Variable(symbol.clone())),
                    },
                });
                (symbol, name)
            })
            .collect::<std::collections::HashMap<_, _>>();
        statements.push(Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body: body
                .iter()
                .map(|statement| rewrite_statement(statement, &replacements))
                .collect(),
        });
        changed = true;
    }

    changed.then(|| {
        let mut hoisted = function.clone();
        hoisted.locals.extend(declarations);
        hoisted.statements = statements;
        hoisted
    })
}

fn retained_call_argument_addresses(
    body: &[Statement],
    source_locals: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut call_ordinal = 0usize;
    let mut candidates = Vec::new();
    for statement in body {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            let Expression::Call { arguments, .. } = expression else {
                return;
            };
            if call_ordinal != 0 {
                for argument in arguments {
                    if let Expression::AddressOf { operand } = argument {
                        if let Expression::Variable(symbol) = operand.as_ref() {
                            if !source_locals.contains(symbol) && !candidates.contains(symbol) {
                                candidates.push(symbol.clone());
                            }
                        }
                    }
                }
            }
            call_ordinal += 1;
        });
    }
    candidates
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next: &mut usize) -> String {
    loop {
        let name = format!("__mwcc_loop_address_{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

fn rewrite_statement(
    statement: &Statement,
    replacements: &std::collections::HashMap<String, String>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: rewrite_expression(target, replacements),
            value: rewrite_expression(value, replacements),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: rewrite_expression(value, replacements),
        },
        Statement::Expression(expression) => {
            Statement::Expression(rewrite_expression(expression, replacements))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: rewrite_expression(condition, replacements),
            then_body: then_body
                .iter()
                .map(|statement| rewrite_statement(statement, replacements))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| rewrite_statement(statement, replacements))
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|value| rewrite_expression(value, replacements)),
        ),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => Statement::Switch {
            scrutinee: rewrite_expression(scrutinee, replacements),
            arms: arms
                .iter()
                .map(|arm| mwcc_syntax_trees::SwitchArm {
                    value: arm.value,
                    body: rewrite_arm(&arm.body, replacements),
                    falls_through: arm.falls_through,
                })
                .collect(),
            default: default
                .as_ref()
                .map(|body| rewrite_arm(body, replacements)),
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
                .map(|value| rewrite_expression(value, replacements)),
            condition: condition
                .as_ref()
                .map(|value| rewrite_expression(value, replacements)),
            step: step
                .as_ref()
                .map(|value| rewrite_expression(value, replacements)),
            body: body
                .iter()
                .map(|statement| rewrite_statement(statement, replacements))
                .collect(),
        },
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => statement.clone(),
    }
}

fn rewrite_arm(
    body: &ArmBody,
    replacements: &std::collections::HashMap<String, String>,
) -> ArmBody {
    match body {
        ArmBody::Return(value) => ArmBody::Return(rewrite_expression(value, replacements)),
        ArmBody::Statements(statements) => ArmBody::Statements(
            statements
                .iter()
                .map(|statement| rewrite_statement(statement, replacements))
                .collect(),
        ),
    }
}

fn rewrite_expression(
    expression: &Expression,
    replacements: &std::collections::HashMap<String, String>,
) -> Expression {
    if let Expression::AddressOf { operand } = expression {
        if let Expression::Variable(symbol) = operand.as_ref() {
            if let Some(name) = replacements.get(symbol) {
                return Expression::Variable(name.clone());
            }
        }
    }
    match expression {
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| rewrite_expression(element, replacements))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(rewrite_expression(left, replacements)),
            right: Box::new(rewrite_expression(right, replacements)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(rewrite_expression(operand, replacements)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(rewrite_expression(condition, replacements)),
            when_true: Box::new(rewrite_expression(when_true, replacements)),
            when_false: Box::new(rewrite_expression(when_false, replacements)),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(rewrite_expression(operand, replacements)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(rewrite_expression(extracted, replacements)),
            promoted_type: *promoted_type,
            storage: Box::new(rewrite_expression(storage, replacements)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(rewrite_expression(value, replacements)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(rewrite_expression(pointer, replacements)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(rewrite_expression(operand, replacements)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(rewrite_expression(base, replacements)),
            index: Box::new(rewrite_expression(index, replacements)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(rewrite_expression(base, replacements)),
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
            base: Box::new(rewrite_expression(base, replacements)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(rewrite_expression(target, replacements)),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, replacements))
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
            object: Box::new(rewrite_expression(object, replacements)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, replacements))
                .collect(),
        },
        Expression::ConstructedNew {
            allocation,
            allocation_size,
            constructor,
            arguments,
        } => Expression::ConstructedNew {
            allocation: Box::new(rewrite_expression(allocation, replacements)),
            allocation_size: *allocation_size,
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, replacements))
                .collect(),
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| rewrite_expression(argument, replacements))
                .collect(),
        },
        Expression::PostStep {
            target,
            operator,
            pointer_link,
        } => Expression::PostStep {
            target: Box::new(rewrite_expression(target, replacements)),
            operator: *operator,
            pointer_link: *pointer_link,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(rewrite_expression(target, replacements)),
            value: Box::new(rewrite_expression(value, replacements)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(rewrite_expression(left, replacements)),
            right: Box::new(rewrite_expression(right, replacements)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}
