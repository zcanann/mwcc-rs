//! Source-proven one-word aggregate storage.
//!
//! A C++ iterator remains an aggregate for overload and member resolution, but
//! an inlined endpoint constructor can prove that its complete runtime value is
//! one pointer word. Those locals may use ordinary register liveness instead of
//! being forced into an addressable aggregate frame slot.

#[allow(unused_imports)]
use super::*;

pub(crate) fn source_proven_one_word_aggregate_locals(
    function: &Function,
) -> std::collections::HashSet<String> {
    function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static
                && !local.is_volatile
                && local.array_length.is_none()
                && matches!(local.declared_type, Type::Struct { size: 4, .. })
                && (local
                    .initializer
                    .as_ref()
                    .is_some_and(is_one_word_aggregate_value)
                    || function
                        .statements
                        .iter()
                        .any(|statement| assigns_one_word_value(statement, &local.name)))
        })
        .map(|local| local.name.clone())
        .collect()
}

fn is_one_word_aggregate_value(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Cast {
            target_type: Type::Struct { size: 4, .. },
            ..
        }
    )
}

fn assigns_one_word_value(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign {
            name: target,
            value,
        } => target == name && is_one_word_aggregate_value(value),
        Statement::Store { target, value } => {
            matches!(target, Expression::Variable(target) if target == name)
                && is_one_word_aggregate_value(value)
        }
        Statement::Expression(expression) => expression_assigns_one_word_value(expression, name),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body
                .iter()
                .any(|inner| assigns_one_word_value(inner, name))
                || else_body
                    .iter()
                    .any(|inner| assigns_one_word_value(inner, name))
        }
        Statement::Loop {
            initializer,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .is_some_and(|value| expression_assigns_one_word_value(value, name))
                || step
                    .as_ref()
                    .is_some_and(|value| expression_assigns_one_word_value(value, name))
                || body
                    .iter()
                    .any(|inner| assigns_one_word_value(inner, name))
        }
        Statement::Switch { arms, .. } => arms.iter().any(|arm| {
            matches!(
                &arm.body,
                mwcc_syntax_trees::ArmBody::Statements(statements)
                    if statements
                        .iter()
                        .any(|inner| assigns_one_word_value(inner, name))
            )
        }),
        Statement::Return(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn expression_assigns_one_word_value(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Assign { target, value } => {
            matches!(target.as_ref(), Expression::Variable(target) if target == name)
                && is_one_word_aggregate_value(value)
        }
        Expression::Comma { left, right } => {
            expression_assigns_one_word_value(left, name)
                || expression_assigns_one_word_value(right, name)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
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

    fn function(local: LocalDeclaration, initializer: Expression) -> Function {
        Function {
            return_type: Type::Void,
            name: "walk".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![local],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("it".into())),
                    value: Box::new(initializer),
                }),
                condition: None,
                step: None,
                body: Vec::new(),
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
        }
    }

    #[test]
    fn requires_both_a_one_word_layout_and_endpoint_marker() {
        let marker = Expression::Cast {
            target_type: Type::Struct { size: 4, align: 4 },
            operand: Box::new(Expression::Variable("pointer".into())),
        };
        let proven = function(
            local("it", Type::Struct { size: 4, align: 4 }),
            marker.clone(),
        );
        let wider = function(
            local("it", Type::Struct { size: 8, align: 4 }),
            marker.clone(),
        );
        let unproven = function(
            local("it", Type::Struct { size: 4, align: 4 }),
            Expression::Variable("pointer".into()),
        );

        assert!(source_proven_one_word_aggregate_locals(&proven).contains("it"));
        assert!(source_proven_one_word_aggregate_locals(&wider).is_empty());
        assert!(source_proven_one_word_aggregate_locals(&unproven).is_empty());
    }
}
