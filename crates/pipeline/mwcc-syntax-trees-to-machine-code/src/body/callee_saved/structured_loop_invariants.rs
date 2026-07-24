//! Source-proven loop-invariant values for structured bodies.
//!
//! Iterator endpoint helpers expose the sentinel as the address of an embedded
//! list anchor. Once the frontend has also proven the iterator's one-word
//! representation, that address is stable for the loop and MWCC retains it in
//! a saved register rather than rebuilding it at every condition check.

#[allow(unused_imports)]
use super::*;

pub(super) fn hoist_iterator_end_sentinels(
    function: &Function,
    one_word_aggregates: &std::collections::HashSet<String>,
) -> Option<Function> {
    let address_taken = crate::frame::collect_address_taken(function);
    let stable_pointer_locals: std::collections::HashSet<String> = function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static
                && !local.is_volatile
                && local.array_length.is_none()
                && matches!(
                    local.declared_type,
                    Type::Pointer(_) | Type::StructPointer { .. }
                )
                && !address_taken.contains(&local.name)
        })
        .map(|local| local.name.clone())
        .collect();
    let mut used_names: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let (statements, changed) = hoist_in_statements(
        &function.statements,
        one_word_aggregates,
        &stable_pointer_locals,
        &mut used_names,
        &mut declarations,
        &mut next_name,
    );
    changed.then(|| {
        let mut hoisted = function.clone();
        hoisted.locals.extend(declarations);
        hoisted.statements = statements;
        hoisted
    })
}

fn hoist_in_statements(
    statements: &[Statement],
    one_word_aggregates: &std::collections::HashSet<String>,
    stable_pointer_locals: &std::collections::HashSet<String>,
    used_names: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(statements.len());
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Loop {
                kind,
                initializer,
                condition: Some(condition),
                step,
                body,
            } if matches!(kind, LoopKind::For | LoopKind::While) => {
                let (body, body_changed) = hoist_in_statements(
                    body,
                    one_word_aggregates,
                    stable_pointer_locals,
                    used_names,
                    declarations,
                    next_name,
                );
                let invariant = hoisted_iterator_condition(
                    condition,
                    one_word_aggregates,
                    stable_pointer_locals,
                )
                .filter(|(_, anchor_base)| {
                    initializer.as_ref().is_none_or(|expression| {
                        !expression_writes_name(expression, anchor_base)
                    }) && step.as_ref().is_none_or(|expression| {
                        !expression_writes_name(expression, anchor_base)
                    }) && !statements_write_name(&body, anchor_base)
                });
                if let Some((endpoint, _)) = invariant {
                    let name = fresh_name(used_names, next_name);
                    declarations.push(pointer_local(&name));
                    let endpoint = Expression::Assign {
                        target: Box::new(Expression::Variable(name.clone())),
                        value: Box::new(endpoint),
                    };
                    let initializer = match initializer {
                        Some(initializer) => Expression::Comma {
                            left: Box::new(initializer.clone()),
                            right: Box::new(endpoint),
                        },
                        None => endpoint,
                    };
                    output.push(Statement::Loop {
                        kind: *kind,
                        initializer: Some(initializer),
                        condition: Some(replace_endpoint(condition, &name)),
                        step: step.clone(),
                        body,
                    });
                    changed = true;
                } else {
                    output.push(Statement::Loop {
                        kind: *kind,
                        initializer: initializer.clone(),
                        condition: Some(condition.clone()),
                        step: step.clone(),
                        body,
                    });
                    changed |= body_changed;
                }
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let (then_body, then_changed) = hoist_in_statements(
                    then_body,
                    one_word_aggregates,
                    stable_pointer_locals,
                    used_names,
                    declarations,
                    next_name,
                );
                let (else_body, else_changed) = hoist_in_statements(
                    else_body,
                    one_word_aggregates,
                    stable_pointer_locals,
                    used_names,
                    declarations,
                    next_name,
                );
                output.push(Statement::If {
                    condition: condition.clone(),
                    then_body,
                    else_body,
                });
                changed |= then_changed || else_changed;
            }
            _ => output.push(statement.clone()),
        }
    }
    (output, changed)
}

fn hoisted_iterator_condition<'a>(
    condition: &'a Expression,
    one_word_aggregates: &std::collections::HashSet<String>,
    stable_pointer_locals: &std::collections::HashSet<String>,
) -> Option<(Expression, &'a str)> {
    let Expression::Binary {
        operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if is_iterator_storage(left, one_word_aggregates) {
        let base = embedded_anchor_base(right)?;
        stable_pointer_locals
            .contains(base)
            .then(|| (right.as_ref().clone(), base))
    } else if is_iterator_storage(right, one_word_aggregates)
    {
        let base = embedded_anchor_base(left)?;
        stable_pointer_locals
            .contains(base)
            .then(|| (left.as_ref().clone(), base))
    } else {
        None
    }
}

fn is_iterator_storage(
    expression: &Expression,
    one_word_aggregates: &std::collections::HashSet<String>,
) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset: 0,
            member_type: Type::Pointer(_) | Type::StructPointer { .. },
            index_stride: None,
        } if matches!(
            base.as_ref(),
            Expression::Variable(name) if one_word_aggregates.contains(name)
        )
    )
}

fn is_embedded_anchor_address(expression: &Expression) -> bool {
    embedded_anchor_base(expression).is_some()
}

fn embedded_anchor_base(expression: &Expression) -> Option<&str> {
    matches!(
        expression,
        Expression::AddressOf {
            operand,
        } if matches!(
            operand.as_ref(),
            Expression::Member {
                base,
                member_type: Type::Struct { .. },
                index_stride: None,
                ..
            } if matches!(base.as_ref(), Expression::Variable(_))
        )
    )
    .then(|| {
        let Expression::AddressOf { operand } = expression else {
            unreachable!()
        };
        let Expression::Member { base, .. } = operand.as_ref() else {
            unreachable!()
        };
        let Expression::Variable(name) = base.as_ref() else {
            unreachable!()
        };
        name.as_str()
    })
}

fn statements_write_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: target, value } => {
            target == name || expression_writes_name(value, name)
        }
        Statement::Store { target, value } => {
            matches!(target, Expression::Variable(target) if target == name)
                || expression_writes_name(target, name)
                || expression_writes_name(value, name)
        }
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            expression_writes_name(expression, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_writes_name(condition, name)
                || statements_write_name(then_body, name)
                || statements_write_name(else_body, name)
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .is_some_and(|expression| expression_writes_name(expression, name))
                || condition
                    .as_ref()
                    .is_some_and(|expression| expression_writes_name(expression, name))
                || step
                    .as_ref()
                    .is_some_and(|expression| expression_writes_name(expression, name))
                || statements_write_name(body, name)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_writes_name(scrutinee, name)
                || arms
                    .iter()
                    .any(|arm| arm_body_writes_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_writes_name(body, name))
        }
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
        Statement::InlineAsm(_) => true,
    })
}

fn arm_body_writes_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(expression) => {
            expression_writes_name(expression, name)
        }
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            statements_write_name(statements, name)
        }
    }
}

fn expression_writes_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Assign { target, value } => {
            matches!(target.as_ref(), Expression::Variable(target) if target == name)
                || expression_writes_name(target, name)
                || expression_writes_name(value, name)
        }
        Expression::Comma { left, right } | Expression::Binary { left, right, .. } => {
            expression_writes_name(left, name) || expression_writes_name(right, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::AddressOf { operand }
        | Expression::Dereference { pointer: operand }
        | Expression::IndexedUpdateValue { value: operand } => {
            expression_writes_name(operand, name)
        }
        Expression::BitFieldRead {
            extracted,
            storage,
            ..
        } => {
            expression_writes_name(extracted, name)
                || expression_writes_name(storage, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_writes_name(condition, name)
                || expression_writes_name(when_true, name)
                || expression_writes_name(when_false, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_writes_name(base, name)
        }
        Expression::Index { base, index } => {
            expression_writes_name(base, name) || expression_writes_name(index, name)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_writes_name(argument, name)),
        Expression::CallThrough { target, arguments } => {
            expression_writes_name(target, name)
                || arguments
                    .iter()
                    .any(|argument| expression_writes_name(argument, name))
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            expression_writes_name(object, name)
                || arguments
                    .iter()
                    .any(|argument| expression_writes_name(argument, name))
        }
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            expression_writes_name(allocation, name)
                || arguments
                    .iter()
                    .any(|argument| expression_writes_name(argument, name))
        }
        Expression::PostStep { target, .. } => {
            matches!(target.as_ref(), Expression::Variable(target) if target == name)
                || expression_writes_name(target, name)
        }
        _ => false,
    }
}

fn replace_endpoint(condition: &Expression, name: &str) -> Expression {
    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        unreachable!("endpoint conditions are binary")
    };
    let replacement = || Box::new(Expression::Variable(name.to_owned()));
    if is_embedded_anchor_address(left) {
        Expression::Binary {
            operator: *operator,
            left: replacement(),
            right: right.clone(),
        }
    } else {
        Expression::Binary {
            operator: *operator,
            left: left.clone(),
            right: replacement(),
        }
    }
}

fn fresh_name(
    used_names: &mut std::collections::HashSet<String>,
    next_name: &mut usize,
) -> String {
    loop {
        let name = format!("__mwcc_iterator_end_{}", *next_name);
        *next_name += 1;
        if used_names.insert(name.clone()) {
            return name;
        }
    }
}

fn pointer_local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::StructPointer { element_size: 0 },
        name: name.to_owned(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoists_a_proven_iterator_anchor_after_the_loop_initializer() {
        let iterator = "it".to_owned();
        let function = Function {
            return_type: Type::Void,
            name: "walk".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                LocalDeclaration {
                    declared_type: Type::Struct { size: 4, align: 4 },
                    name: iterator.clone(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                },
                pointer_local("list"),
            ],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Member {
                        base: Box::new(Expression::Variable(iterator.clone())),
                        offset: 0,
                        member_type: Type::StructPointer { element_size: 0 },
                        index_stride: None,
                    }),
                    right: Box::new(Expression::AddressOf {
                        operand: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("list".into())),
                            offset: 4,
                            member_type: Type::Struct { size: 8, align: 4 },
                            index_stride: None,
                        }),
                    }),
                }),
                step: None,
                body: vec![Statement::Expression(Expression::Call {
                    name: "visit".into(),
                    arguments: Vec::new(),
                })],
            }],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let mut reassigned_anchor = function.clone();
        let Statement::Loop { body, .. } = &mut reassigned_anchor.statements[0] else {
            unreachable!()
        };
        body.push(Statement::Assign {
            name: "list".into(),
            value: Expression::Variable("replacement".into()),
        });
        assert!(
            hoist_iterator_end_sentinels(
                &reassigned_anchor,
                &std::collections::HashSet::from([iterator.clone()]),
            )
            .is_none(),
            "a loop-mutated anchor is not invariant"
        );

        let hoisted = hoist_iterator_end_sentinels(
            &function,
            &std::collections::HashSet::from([iterator]),
        )
        .expect("proven endpoint should hoist");

        assert_eq!(hoisted.locals.len(), 3);
        let [Statement::Loop {
            initializer: Some(Expression::Assign { target, value }),
            condition: Some(Expression::Binary { right, .. }),
            ..
        }] = hoisted.statements.as_slice()
        else {
            panic!("the endpoint assignment should become the loop initializer")
        };
        assert!(matches!(target.as_ref(), Expression::Variable(name)
            if matches!(right.as_ref(), Expression::Variable(right_name) if right_name == name)));
        assert!(matches!(value.as_ref(), Expression::AddressOf { .. }));
    }
}
