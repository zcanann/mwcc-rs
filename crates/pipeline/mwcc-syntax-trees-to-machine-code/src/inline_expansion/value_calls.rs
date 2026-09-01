//! Recursive substitution of expression-valued retained inline calls.

use super::safety::{stable_argument, stable_local_values, stable_lvalue_address};
use super::substitution::substitute_expression;
use super::value_body::ValueInlineBody;
use mwcc_syntax_trees::{
    ArmBody, BinaryOperator, Expression, LocalDeclaration, Statement, Type, UnaryOperator,
};
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
    function_symbols: &HashSet<String>,
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
            function_symbols,
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
        Statement::Assign { name, value } => {
            let value = expression(value, active, changed, value_body_substitutions);
            compose_conditional_assignment(name, &value, bodies).unwrap_or_else(|| {
                Statement::Assign {
                    name: name.clone(),
                    value,
                }
            })
        }
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
                        function_symbols,
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
                        function_symbols,
                        active,
                        changed,
                        value_body_substitutions,
                        allocator,
                    )
                })
                .collect();
            compose_guarded_truth_value(condition, then_body, else_body, bodies)
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
                        function_symbols,
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
                    function_symbols,
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
                        function_symbols,
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

/// Turn a value-inline early-return selection back into an assignment diamond.
/// Prefix effects (normally hygienic argument captures) remain sequenced in
/// the condition, while each arm owns its effects and final destination value.
fn compose_conditional_assignment(
    destination: &str,
    value: &Expression,
    bodies: &HashMap<String, ValueInlineBody>,
) -> Option<Statement> {
    let mut prefix = Vec::new();
    let mut selection = value;
    while let Expression::Comma { left, right } = selection {
        prefix.push(left.as_ref().clone());
        selection = right;
    }
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        origin: mwcc_syntax_trees::ConditionalOrigin::IfReturns,
    } = selection
    else {
        return None;
    };
    let condition = prefix.into_iter().rev().fold(
        condition.as_ref().clone(),
        |right, left| Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        },
    );
    Some(Statement::If {
        condition,
        then_body: assignment_arm_statements(destination, when_true, bodies)?,
        else_body: assignment_arm_statements(destination, when_false, bodies)?,
    })
}

fn assignment_arm_statements(
    destination: &str,
    expression: &Expression,
    bodies: &HashMap<String, ValueInlineBody>,
) -> Option<Vec<Statement>> {
    let mut sequence = Vec::new();
    flatten_sequence(expression, &mut sequence);
    let value = sequence.pop()?;
    let mut statements = sequence
        .into_iter()
        .map(|effect| effect_statement(effect, bodies))
        .collect::<Option<Vec<_>>>()?;
    statements.push(Statement::Assign {
        name: destination.to_owned(),
        value,
    });
    Some(statements)
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
    mut else_body: Vec<Statement>,
    bodies: &HashMap<String, ValueInlineBody>,
) -> Statement {
    let Some((guard, effects, caller_true_on_guard)) = guarded_boolean_effects(&condition) else {
        return Statement::If {
            condition,
            then_body,
            else_body,
        };
    };
    let mut composed = effects
        .into_iter()
        .filter_map(|effect| effect_statement(effect, bodies))
        .collect::<Vec<_>>();
    if caller_true_on_guard {
        composed.append(&mut then_body);
        Statement::If {
            condition: guard.clone(),
            then_body: composed,
            else_body,
        }
    } else {
        composed.append(&mut else_body);
        Statement::If {
            condition: guard.clone(),
            then_body: composed,
            else_body: then_body,
        }
    }
}

fn guarded_boolean_effects(
    condition: &Expression,
) -> Option<(&Expression, Vec<Expression>, bool)> {
    let (selection, caller_true_on_guard) = match condition {
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => (left.as_ref(), true),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(left.as_ref(), Expression::IntegerLiteral(0)) => (right.as_ref(), true),
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => {
            return guarded_selection_effects(left).map(|(guard, effects)| {
                (guard, effects, false)
            });
        }
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if matches!(left.as_ref(), Expression::IntegerLiteral(0)) => {
            return guarded_selection_effects(right).map(|(guard, effects)| {
                (guard, effects, false)
            });
        }
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => {
            return guarded_selection_effects(operand)
                .map(|(guard, effects)| (guard, effects, false));
        }
        expression => (expression, true),
    };
    guarded_selection_effects(selection)
        .map(|(guard, effects)| (guard, effects, caller_true_on_guard))
}

fn guarded_selection_effects(
    selection: &Expression,
) -> Option<(&Expression, Vec<Expression>)> {
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

fn effect_statement(
    expression: Expression,
    bodies: &HashMap<String, ValueInlineBody>,
) -> Option<Statement> {
    match expression {
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => Some(Statement::If {
            condition: *condition,
            then_body: effect_arm_statements(*when_true, bodies)?,
            else_body: effect_arm_statements(*when_false, bodies)?,
        }),
        Expression::Assign { target, value } => match *target {
            Expression::Variable(name)
                if bodies.values().any(|body| body.stores_global_name(&name)) =>
            {
                Some(Statement::Store {
                    target: Expression::Variable(name),
                    value: *value,
                })
            }
            Expression::Variable(name) => {
                Some(Statement::Assign { name, value: *value })
            }
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

fn effect_arm_statements(
    expression: Expression,
    bodies: &HashMap<String, ValueInlineBody>,
) -> Option<Vec<Statement>> {
    let mut sequence = Vec::new();
    flatten_sequence(&expression, &mut sequence);
    if !matches!(sequence.pop(), Some(Expression::IntegerLiteral(0))) {
        return None;
    }
    sequence
        .into_iter()
        .map(|effect| effect_statement(effect, bodies))
        .collect()
}

fn expand_arm(
    body: &ArmBody,
    bodies: &HashMap<String, ValueInlineBody>,
    stable_variables: &HashSet<String>,
    function_symbols: &HashSet<String>,
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
            function_symbols,
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
                        function_symbols,
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
    function_symbols: &HashSet<String>,
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
            function_symbols,
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
            // Automatic transaction selection is a unit-front decision. A
            // caller may inline a wrapper's source body while that wrapper's
            // separately emitted definition expands its own transaction, but
            // MWCC does not recursively re-run transaction selection inside
            // the newly substituted wrapper body.
            if !active.is_empty()
                && (body.automatic_transaction
                    || super::value_body::summarize_automatic_transaction(&body.source).is_some())
            {
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
                let guarded_transaction_single_use =
                    super::value_body::summarize_automatic_guarded_transaction(&body.source)
                        .is_some()
                        && use_count == 1
                        && matches!(
                            argument,
                            Expression::Variable(_)
                                | Expression::IntegerLiteral(_)
                                | Expression::FloatLiteral(_)
                        );
                let known_function_designator = body.forwards_known_function_designators()
                    && matches!(
                        &argument,
                        Expression::Variable(name) if function_symbols.contains(name)
                    );
                let aggregate_reference_lvalue = matches!(
                    parameter.parameter_type,
                    Type::StructPointer { .. }
                ) && match &argument {
                    Expression::Variable(name) => allocator.locals.iter().any(|local| {
                        local.name == *name && matches!(local.declared_type, Type::Struct { .. })
                    }),
                    Expression::Member { member_type, .. } => {
                        matches!(member_type, Type::Struct { .. })
                            && stable_lvalue_address(&argument, stable_variables)
                    }
                    _ => false,
                };
                // A bare variable is safe to substitute without proving caller
                // stability only when the inline body reads it once. Repeating
                // the expression can reread a volatile global (and can observe a
                // changed local), whereas an ordinary call captures its argument
                // value exactly once before entering the callee.
                let read_only_variable = use_count == 1
                    && matches!(argument, Expression::Variable(_))
                    && !super::safety::parameter_requires_materialization(
                        &body.source,
                        &parameter.name,
                    )
                    && !crate::analysis::expression_has_call(&body.expression);
                if forwards_once_in_order
                    || pure_single_use
                    || guarded_transaction_single_use
                    || known_function_designator
                    || aggregate_reference_lvalue
                    || read_only_variable
                    || stable_argument(&argument, stable_variables)
                {
                    let replacement = if aggregate_reference_lvalue {
                        Expression::AddressOf {
                            operand: Box::new(argument),
                        }
                    } else {
                        argument
                    };
                    replacements.insert(parameter.name.clone(), replacement);
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
                    attribute_alignment: None,
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
                function_symbols,
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
        } => apply_cast_to_terminal_value(
            *target_type,
            recurse(operand, active, changed, value_body_substitutions),
        ),
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
        Expression::AddressOf { operand } => {
            let operand = recurse(operand, active, changed, value_body_substitutions);
            // A C++ reference-returning inline is summarized as the explicit
            // address of its source lvalue. When the caller wrote `&get()`,
            // composition would otherwise manufacture `&&member`; retain the
            // single pointer-valued address that the non-inlined call returns.
            if let Expression::AddressOf { .. } = operand {
                operand
            } else {
                Expression::AddressOf {
                    operand: Box::new(operand),
                }
            }
        }
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

/// Keep inline setup effects visible to value-position lowering.
///
/// A cast applies only to the comma operator's surviving right value. Sinking
/// it through the sequence is therefore semantic normalization, and avoids
/// hiding the effect prefix in a sub-expression position that would require an
/// artificial result move.
fn apply_cast_to_terminal_value(target_type: Type, operand: Expression) -> Expression {
    match operand {
        Expression::Comma { left, right } => Expression::Comma {
            left,
            right: Box::new(apply_cast_to_terminal_value(target_type, *right)),
        },
        operand => Expression::Cast {
            target_type,
            operand: Box::new(operand),
        },
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

    fn empty_function(name: &str, return_type: Type) -> mwcc_syntax_trees::Function {
        mwcc_syntax_trees::Function {
            return_type,
            name: name.into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn captures_a_repeated_unstable_variable_argument_once() {
        let mut source = empty_function("predicate", Type::Int);
        source.parameters.push(mwcc_syntax_trees::Parameter {
            parameter_type: Type::UnsignedInt,
            name: "command".into(),
        });
        let parameter = || Expression::Variable("command".into());
        let body = ValueInlineBody {
            source,
            expression: Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(parameter()),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(parameter()),
                    right: Box::new(Expression::IntegerLiteral(4)),
                }),
            },
            automatic_transaction: false,
        };
        let bodies = HashMap::from([("predicate".into(), body)]);
        let call = Expression::Call {
            name: "predicate".into(),
            arguments: vec![Expression::Variable("volatile_command".into())],
        };
        let mut locals = Vec::new();
        let mut occupied_names = HashSet::new();
        let mut next_local_id = 0;
        let mut allocator = LocalAllocator {
            locals: &mut locals,
            occupied_names: &mut occupied_names,
            next_local_id: &mut next_local_id,
        };
        let mut active = HashSet::new();
        let mut changed = false;
        let mut substitutions = 0;

        let expanded = expand_expression(
            &call,
            &bodies,
            &HashSet::new(),
            &HashSet::new(),
            &mut active,
            &mut changed,
            &mut substitutions,
            &mut allocator,
        );

        assert!(changed);
        assert_eq!(substitutions, 1);
        assert_eq!(locals.len(), 1);
        assert_eq!(
            super::super::safety::expression_use_count(&expanded, "volatile_command"),
            1
        );
        assert_eq!(
            super::super::safety::expression_use_count(&expanded, &locals[0].name),
            3
        );
    }

    #[test]
    fn aliases_an_aggregate_lvalue_for_a_repeated_reference_parameter() {
        let mut source = empty_function("set_length", Type::Float);
        source.parameters.push(mwcc_syntax_trees::Parameter {
            parameter_type: Type::StructPointer { element_size: 12 },
            name: "other".into(),
        });
        let body = ValueInlineBody {
            source,
            expression: Expression::Call {
                name: "consume".into(),
                arguments: vec![
                    Expression::Variable("other".into()),
                    Expression::AddressOf {
                        operand: Box::new(Expression::Variable("other".into())),
                    },
                ],
            },
            automatic_transaction: false,
        };
        let bodies = HashMap::from([("set_length".into(), body)]);
        let call = Expression::Call {
            name: "set_length".into(),
            arguments: vec![Expression::Variable("vector".into())],
        };
        let mut locals = vec![LocalDeclaration {
            declared_type: Type::Struct { size: 12, align: 4 },
            name: "vector".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }];
        let mut occupied_names = HashSet::from(["vector".into()]);
        let mut next_local_id = 0;
        let mut allocator = LocalAllocator {
            locals: &mut locals,
            occupied_names: &mut occupied_names,
            next_local_id: &mut next_local_id,
        };
        let mut active = HashSet::new();
        let mut changed = false;
        let mut substitutions = 0;

        let expanded = expand_expression(
            &call,
            &bodies,
            &HashSet::new(),
            &HashSet::new(),
            &mut active,
            &mut changed,
            &mut substitutions,
            &mut allocator,
        );

        assert!(changed);
        assert_eq!(substitutions, 1);
        assert_eq!(locals.len(), 1, "reference binding must not copy the aggregate");
        assert_eq!(
            super::super::safety::expression_use_count(&expanded, "vector"),
            2
        );
        assert!(matches!(
            expanded,
            Expression::Call { arguments, .. }
                if arguments.iter().all(|argument| matches!(
                    argument,
                    Expression::AddressOf { operand }
                        if matches!(operand.as_ref(), Expression::Variable(name)
                            if name == "vector")
                ))
        ));
    }

    #[test]
    fn sinks_a_cast_onto_a_comma_sequences_terminal_value() {
        let normalized = apply_cast_to_terminal_value(
            Type::Pointer(mwcc_syntax_trees::Pointee::Char),
            Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("temporary".into())),
                    value: Box::new(Expression::IntegerLiteral(7)),
                }),
                right: Box::new(Expression::Variable("temporary".into())),
            },
        );

        assert!(matches!(
            normalized,
            Expression::Comma { left, right }
                if matches!(left.as_ref(), Expression::Assign { .. })
                    && matches!(right.as_ref(), Expression::Cast {
                        target_type: Type::Pointer(mwcc_syntax_trees::Pointee::Char),
                        operand,
                    } if matches!(operand.as_ref(), Expression::Variable(name)
                        if name == "temporary"))
        ));
    }

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
            &HashMap::new(),
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

    #[test]
    fn composes_a_false_result_consumer_onto_the_guard_else_edge() {
        let flag = Expression::Variable("flag".into());
        let expanded = Expression::Binary {
            operator: BinaryOperator::Equal,
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
            vec![Statement::Expression(Expression::Variable("not_requested".into()))],
            Vec::new(),
            &HashMap::new(),
        );

        assert!(matches!(
            composed,
            Statement::If {
                condition: Expression::Variable(ref name),
                ref then_body,
                ref else_body,
            } if name == "flag"
                && matches!(then_body.as_slice(), [
                    Statement::Assign { name, value: Expression::IntegerLiteral(0) },
                ] if name == "flag")
                && matches!(else_body.as_slice(), [
                    Statement::Expression(Expression::Variable(body)),
                ] if body == "not_requested")
        ));
    }
}
