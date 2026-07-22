//! Hidden frame temporaries for scalar reads from aggregate call results.
//!
//! The EABI returns aggregates through a caller-provided address. Source such
//! as `object->position().x` has no named automatic, so make that address
//! explicit in the semantic tree before frame planning. Each source occurrence
//! receives its own semantic temporary; frame-slot lifetime coloring may safely
//! coalesce them later, while distinct slots keep sibling call results alive.

use super::*;

pub(super) fn materialize_aggregate_return_temporaries(function: &Function) -> Option<Function> {
    let mut rewritten = function.clone();
    let mut names: std::collections::HashSet<String> = rewritten
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(rewritten.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_temporary = 0usize;
    let mut added = Vec::new();
    let mut changed = false;

    for local in &mut rewritten.locals {
        if let Some(initializer) = &mut local.initializer {
            changed |= rewrite_expression(
                initializer,
                &mut names,
                &mut next_temporary,
                &mut added,
            );
        }
    }
    changed |= rewrite_statements(
        &mut rewritten.statements,
        &mut names,
        &mut next_temporary,
        &mut added,
    );
    for guard in &mut rewritten.guards {
        changed |= rewrite_expression(
            &mut guard.condition,
            &mut names,
            &mut next_temporary,
            &mut added,
        );
        changed |= rewrite_expression(
            &mut guard.value,
            &mut names,
            &mut next_temporary,
            &mut added,
        );
    }
    if let Some(value) = &mut rewritten.return_expression {
        changed |= rewrite_expression(value, &mut names, &mut next_temporary, &mut added);
    }
    if !changed {
        return None;
    }
    rewritten.locals.extend(added);
    Some(rewritten)
}

fn rewrite_statements(
    statements: &mut [Statement],
    names: &mut std::collections::HashSet<String>,
    next_temporary: &mut usize,
    added: &mut Vec<LocalDeclaration>,
) -> bool {
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Store { target, value } => {
                changed |= rewrite_expression(target, names, next_temporary, added);
                changed |= rewrite_expression(value, names, next_temporary, added);
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => {
                changed |= rewrite_expression(value, names, next_temporary, added);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                changed |= rewrite_expression(condition, names, next_temporary, added);
                changed |= rewrite_statements(then_body, names, next_temporary, added);
                changed |= rewrite_statements(else_body, names, next_temporary, added);
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                for expression in initializer.iter_mut().chain(condition).chain(step) {
                    changed |= rewrite_expression(expression, names, next_temporary, added);
                }
                changed |= rewrite_statements(body, names, next_temporary, added);
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                changed |= rewrite_expression(scrutinee, names, next_temporary, added);
                for arm in arms {
                    changed |= rewrite_arm(&mut arm.body, names, next_temporary, added);
                }
                if let Some(default) = default {
                    changed |= rewrite_arm(default, names, next_temporary, added);
                }
            }
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => {}
        }
    }
    changed
}

fn rewrite_arm(
    arm: &mut mwcc_syntax_trees::ArmBody,
    names: &mut std::collections::HashSet<String>,
    next_temporary: &mut usize,
    added: &mut Vec<LocalDeclaration>,
) -> bool {
    match arm {
        mwcc_syntax_trees::ArmBody::Return(value) => {
            rewrite_expression(value, names, next_temporary, added)
        }
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            rewrite_statements(statements, names, next_temporary, added)
        }
    }
}

fn rewrite_expression(
    expression: &mut Expression,
    names: &mut std::collections::HashSet<String>,
    next_temporary: &mut usize,
    added: &mut Vec<LocalDeclaration>,
) -> bool {
    let mut changed = match expression {
        Expression::AggregateLiteral(elements) => elements
            .iter_mut()
            .fold(false, |changed, element| {
                rewrite_expression(element, names, next_temporary, added) || changed
            }),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            let left_changed = rewrite_expression(left, names, next_temporary, added);
            let right_changed = rewrite_expression(right, names, next_temporary, added);
            left_changed || right_changed
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            let condition_changed = rewrite_expression(condition, names, next_temporary, added);
            let true_changed = rewrite_expression(when_true, names, next_temporary, added);
            let false_changed = rewrite_expression(when_false, names, next_temporary, added);
            condition_changed || true_changed || false_changed
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::PostStep {
            target: operand, ..
        }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => {
            rewrite_expression(operand, names, next_temporary, added)
        }
        Expression::Index { base, index } => {
            let base_changed = rewrite_expression(base, names, next_temporary, added);
            let index_changed = rewrite_expression(index, names, next_temporary, added);
            base_changed || index_changed
        }
        Expression::Call { arguments, .. }
        | Expression::ConstructedNew { arguments, .. } => arguments
            .iter_mut()
            .fold(false, |changed, argument| {
                rewrite_expression(argument, names, next_temporary, added) || changed
            }),
        Expression::CallThrough { target, arguments } => {
            let target_changed = rewrite_expression(target, names, next_temporary, added);
            let arguments_changed = arguments
                .iter_mut()
                .fold(false, |changed, argument| {
                    rewrite_expression(argument, names, next_temporary, added) || changed
                });
            target_changed || arguments_changed
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            let object_changed = rewrite_expression(object, names, next_temporary, added);
            let arguments_changed = arguments
                .iter_mut()
                .fold(false, |changed, argument| {
                    rewrite_expression(argument, names, next_temporary, added) || changed
                });
            object_changed || arguments_changed
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    };

    let replacement = match expression {
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } if !matches!(member_type, Type::Struct { .. }) => {
            let Expression::VirtualCall {
                return_type: Type::Struct { size, align },
                ..
            } = base.as_ref()
            else {
                return changed;
            };
            let stem = format!(
                "__mwcc_aggregate_result_{}_{}_{}",
                size, align, *next_temporary
            );
            *next_temporary += 1;
            let mut temporary = stem.clone();
            let mut suffix = 0usize;
            while names.contains(&temporary) {
                suffix += 1;
                temporary = format!("{stem}_{suffix}");
            }
            names.insert(temporary.clone());
            added.push(LocalDeclaration {
                declared_type: Type::Struct {
                    size: *size,
                    align: *align,
                },
                name: temporary.clone(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            });
            let call = base.as_ref().clone();
            Some(Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable(temporary.clone())),
                    value: Box::new(call),
                }),
                right: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable(temporary)),
                    offset: *offset,
                    member_type: *member_type,
                    index_stride: None,
                }),
            })
        }
        _ => None,
    };
    if let Some(replacement) = replacement {
        *expression = replacement;
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aggregate_member(object: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::VirtualCall {
                object: Box::new(Expression::Variable(object.into())),
                vptr_offset: 0,
                slot_offset: 40,
                return_type: Type::Struct { size: 12, align: 4 },
                variadic: false,
                arguments: Vec::new(),
            }),
            offset,
            member_type: Type::Float,
            index_stride: None,
        }
    }

    #[test]
    fn gives_sibling_aggregate_calls_distinct_hidden_results() {
        let function = Function {
            return_type: Type::Float,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(aggregate_member("first", 0)),
                right: Box::new(aggregate_member("second", 8)),
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let rewritten = materialize_aggregate_return_temporaries(&function)
            .expect("aggregate member calls need hidden results");
        assert_eq!(rewritten.locals.len(), 2);
        assert_ne!(rewritten.locals[0].name, rewritten.locals[1].name);
        assert!(rewritten.locals.iter().all(|local| {
            local.declared_type == (Type::Struct { size: 12, align: 4 })
                && local.initializer.is_none()
        }));
        assert!(matches!(
            rewritten.return_expression,
            Some(Expression::Binary { left, right, .. })
                if matches!(left.as_ref(), Expression::Comma { .. })
                    && matches!(right.as_ref(), Expression::Comma { .. })
        ));
    }
}
