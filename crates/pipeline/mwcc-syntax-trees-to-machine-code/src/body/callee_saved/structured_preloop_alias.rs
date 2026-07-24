//! Stable pointer aliases introduced by inlined accessors before a loop.
//!
//! An inlined getter commonly leaves `alias = (setup, &object.member)`. When
//! the alias is assigned once and only consumed afterward, preserve `setup` at
//! its source position and substitute the derived address into the following
//! loop. Member lowering can then combine the accessor and endpoint offsets.

#[allow(unused_imports)]
use super::*;

pub(super) fn fold_preloop_comma_pointer_alias(function: &Function) -> Option<Function> {
    let address_taken = crate::frame::collect_address_taken(function);
    for (index, statement) in function.statements.iter().enumerate() {
        let Statement::Assign { name, value } = statement else {
            continue;
        };
        let Some(local) = function.locals.iter().find(|local| local.name == *name) else {
            continue;
        };
        if local.is_static
            || local.is_volatile
            || local.array_length.is_some()
            || local.initializer.is_some()
            || !matches!(
                local.declared_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || address_taken.contains(name)
        {
            continue;
        }
        let Expression::Comma { left, right } = value else {
            continue;
        };
        if !is_embedded_address(right)
            || function.statements[..index]
                .iter()
                .any(|statement| statement_mentions(statement, name))
            || function.statements[index + 1..]
                .iter()
                .any(|statement| statement_assigns(statement, name))
            || !function.statements[index + 1..]
                .iter()
                .any(|statement| statement_mentions(statement, name))
        {
            continue;
        }

        let values = std::collections::HashMap::from([(name.clone(), right.as_ref().clone())]);
        let mut statements = Vec::with_capacity(function.statements.len());
        statements.extend_from_slice(&function.statements[..index]);
        statements.push(effect_statement(left));
        statements.extend(
            function.statements[index + 1..]
                .iter()
                .map(|statement| super::super::passes::substitute_statement(statement, &values)),
        );
        let mut folded = function.clone();
        folded.locals.retain(|candidate| candidate.name != *name);
        folded.statements = statements;
        return Some(folded);
    }
    None
}

fn effect_statement(expression: &Expression) -> Statement {
    if let Expression::Assign { target, value } = expression {
        if let Expression::Variable(name) = target.as_ref() {
            return Statement::Assign {
                name: name.clone(),
                value: value.as_ref().clone(),
            };
        }
    }
    Statement::Expression(expression.clone())
}

fn is_embedded_address(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::AddressOf { operand }
            if matches!(
                operand.as_ref(),
                Expression::Member {
                    member_type: Type::Struct { .. },
                    index_stride: None,
                    ..
                }
            )
    )
}

fn statement_mentions(statement: &Statement, name: &str) -> bool {
    super::structured_locals::body_uses_local(std::slice::from_ref(statement), name)
}

fn statement_assigns(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            crate::analysis::expression_assigns_name(expression, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::expression_assigns_name(condition, name)
                || then_body
                    .iter()
                    .any(|statement| statement_assigns(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_assigns(statement, name))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .iter()
                .chain(condition)
                .chain(step)
                .any(|expression| crate::analysis::expression_assigns_name(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_assigns(statement, name))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 0 },
            name: name.into(),
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

    #[test]
    fn preserves_comma_setup_and_substitutes_the_embedded_address_into_a_loop() {
        let function = Function {
            return_type: Type::Void,
            name: "walk".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![pointer_local("list"), pointer_local("owner")],
            statements: vec![
                Statement::Assign {
                    name: "list".into(),
                    value: Expression::Comma {
                        left: Box::new(Expression::Assign {
                            target: Box::new(Expression::Variable("owner".into())),
                            value: Box::new(Expression::Variable("source".into())),
                        }),
                        right: Box::new(Expression::AddressOf {
                            operand: Box::new(Expression::Member {
                                base: Box::new(Expression::Variable("owner".into())),
                                offset: 12,
                                member_type: Type::Struct { size: 12, align: 4 },
                                index_stride: None,
                            }),
                        }),
                    },
                },
                Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::Member {
                        base: Box::new(Expression::Variable("list".into())),
                        offset: 4,
                        member_type: Type::Int,
                        index_stride: None,
                    }),
                    step: None,
                    body: Vec::new(),
                },
            ],
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

        let folded = fold_preloop_comma_pointer_alias(&function).expect("stable alias");

        assert!(!folded.locals.iter().any(|local| local.name == "list"));
        assert!(matches!(
            folded.statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Loop {
                    condition: Some(Expression::Member { base, .. }),
                    ..
                },
            ] if name == "owner" && matches!(base.as_ref(), Expression::AddressOf { .. })
        ));
    }
}
