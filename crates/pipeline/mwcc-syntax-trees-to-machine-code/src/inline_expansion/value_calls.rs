//! Recursive substitution of expression-valued retained inline calls.

use super::safety::{stable_argument, stable_local_values};
use super::substitution::substitute_expression;
use super::value_body::ValueInlineBody;
use mwcc_syntax_trees::{ArmBody, BinaryOperator, Expression, LocalDeclaration, Statement};
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
        Statement::InlineAsm(_) => statement.clone(),
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
        } => {
            let condition = expression(condition, active, changed, value_body_substitutions);
            let then_body = then_body
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
                .collect();
            let else_body = else_body
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
                .collect();
            compose_guarded_truth_value(condition, then_body, else_body)
        }
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

/// Turn an expression-valued flag consumer back into the source-level branch
/// shape when its selected true value owns side effects.
///
/// Retained inline calls are summarized as expressions because they can occur
/// anywhere. At an enclosing `if`, however, `flag ? (clear, 1) : 0` has a more
/// faithful statement representation: test `flag`, then clear it before the
/// caller's true body. Keeping this rewrite at the statement-composition
/// boundary avoids teaching low-level select emission that an arm effect owns
/// the caller's control-flow edge.
fn compose_guarded_truth_value(
    condition: Expression,
    mut then_body: Vec<Statement>,
    else_body: Vec<Statement>,
) -> Statement {
    let Some((guard, effects)) = guarded_true_effects(&condition) else {
        return Statement::If {
            condition,
            then_body,
            else_body,
        };
    };
    let mut composed = effects
        .into_iter()
        .filter_map(effect_statement)
        .collect::<Vec<_>>();
    composed.append(&mut then_body);
    Statement::If {
        condition: guard.clone(),
        then_body: composed,
        else_body,
    }
}

fn guarded_true_effects(condition: &Expression) -> Option<(&Expression, Vec<Expression>)> {
    let selection = match condition {
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => left.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(left.as_ref(), Expression::IntegerLiteral(0)) => right.as_ref(),
        expression => expression,
    };
    let Expression::Conditional {
        condition: guard,
        when_true,
        when_false,
        ..
    } = selection
    else {
        return None;
    };
    if !matches!(when_false.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let mut sequence = Vec::new();
    flatten_sequence(when_true, &mut sequence);
    if !matches!(sequence.pop(), Some(Expression::IntegerLiteral(1))) || sequence.is_empty() {
        return None;
    }
    Some((guard, sequence))
}

fn flatten_sequence(expression: &Expression, output: &mut Vec<Expression>) {
    match expression {
        Expression::Comma { left, right } => {
            flatten_sequence(left, output);
            flatten_sequence(right, output);
        }
        expression => output.push(expression.clone()),
    }
}

fn effect_statement(expression: Expression) -> Option<Statement> {
    match expression {
        Expression::Assign { target, value } => match *target {
            Expression::Variable(name) => Some(Statement::Assign {
                name,
                value: *value,
            }),
            target => Some(Statement::Store {
                target,
                value: *value,
            }),
        },
        Expression::Variable(_)
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => None,
        expression => Some(Statement::Expression(expression)),
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
            let forwards_once_in_order = body.arguments_forwarded_once_in_order();
            for (parameter, argument) in body.source.parameters.iter().zip(arguments) {
                let use_count =
                    super::safety::expression_use_count(&body.expression, &parameter.name);
                if use_count == 0 {
                    if crate::analysis::expression_has_side_effect(&argument) {
                        argument_initializers.push(argument);
                    }
                    continue;
                }
                let pure_single_use = !crate::analysis::expression_has_side_effect(&argument)
                    && body.parameter_used_once_in_forwarded_call(&parameter.name);
                if forwards_once_in_order
                    || pure_single_use
                    || stable_argument(&argument, stable_variables)
                {
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
            allocation,
            allocation_size,
            constructor,
            arguments,
        } => Expression::ConstructedNew {
            allocation: Box::new(recurse(
                allocation,
                active,
                changed,
                value_body_substitutions,
            )),
            allocation_size: *allocation_size,
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|argument| recurse(argument, active, changed, value_body_substitutions))
                .collect(),
        },
        Expression::PostStep {
            target,
            operator,
            pointer_link,
        } => Expression::PostStep {
            target: Box::new(recurse(target, active, changed, value_body_substitutions)),
            operator: *operator,
            pointer_link: *pointer_link,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_a_true_arm_effect_into_the_enclosing_if() {
        let flag = Expression::Variable("flag".into());
        let expanded = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Conditional {
                condition: Box::new(flag.clone()),
                when_true: Box::new(Expression::Comma {
                    left: Box::new(Expression::Assign {
                        target: Box::new(flag.clone()),
                        value: Box::new(Expression::IntegerLiteral(0)),
                    }),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
                when_false: Box::new(Expression::IntegerLiteral(0)),
                origin: mwcc_syntax_trees::ConditionalOrigin::IfReturns,
            }),
            right: Box::new(Expression::IntegerLiteral(0)),
        };

        let composed = compose_guarded_truth_value(
            expanded,
            vec![Statement::Expression(Expression::Variable("body".into()))],
            Vec::new(),
        );
        assert!(matches!(
            composed,
            Statement::If {
                condition: Expression::Variable(ref name),
                ref then_body,
                ..
            } if name == "flag"
                && matches!(then_body.as_slice(), [
                    Statement::Assign { name, value: Expression::IntegerLiteral(0) },
                    Statement::Expression(Expression::Variable(body)),
                ] if name == "flag" && body == "body")
        ));
    }
}
