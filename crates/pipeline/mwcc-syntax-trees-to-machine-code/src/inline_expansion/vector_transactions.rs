//! Automatic-inline summaries for compact three-component vector helpers.

use super::value_body::{self, ValueInlineBody};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::HashSet;

/// Recognize the semantic transaction MWCC commonly composes when a
/// three-float aggregate is passed by value into a pure scalar projection.
pub(super) fn summarize_automatic(function: &Function) -> Option<ValueInlineBody> {
    if function.return_type != Type::Float
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || function.parameters.iter().any(|parameter| {
            parameter.parameter_type == Type::Void
                || parameter.parameter_type == Type::Double
        })
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || local.array_length.is_some()
                || local.declared_type != Type::Float
        })
    {
        return None;
    }
    if !is_scalar_projection(function) {
        return None;
    }
    value_body::summarize(function)
}

fn is_scalar_projection(function: &Function) -> bool {
    let local_names = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    function.parameters.len() == 3
        && function
            .parameters
            .iter()
            .all(|parameter| is_three_float_struct(parameter.parameter_type))
        && function.locals.len() == 2
        && matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name))
                if function.locals.iter().any(|local| local.name == *name)
        )
        && matches!(
            function.statements.as_slice(),
            [Statement::Assign { .. }, Statement::Assign { .. }, Statement::If { .. }]
        )
        && projection_statements(&function.statements, &local_names)
}

fn projection_statements(statements: &[Statement], local_names: &HashSet<&str>) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { name, value } => {
            local_names.contains(name.as_str())
                && !crate::analysis::expression_has_side_effect(value)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            !crate::analysis::expression_has_side_effect(condition)
                && projection_statements(then_body, local_names)
                && projection_statements(else_body, local_names)
        }
        _ => false,
    })
}

fn is_three_float_struct(value_type: Type) -> bool {
    matches!(value_type, Type::Struct { size: 12, .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, LocalDeclaration, Parameter};

    #[test]
    fn summarizes_a_pure_three_float_projection() {
        let vector = Type::Struct { size: 12, align: 4 };
        let mut function = Function {
            return_type: Type::Float,
            name: "project".into(),
            is_static: false,
            is_weak: false,
            parameters: ["first", "second", "axis"]
                .into_iter()
                .map(|name| Parameter {
                    parameter_type: vector,
                    name: name.into(),
                })
                .collect(),
            locals: ["numerator", "denominator"]
                .into_iter()
                .map(|name| LocalDeclaration {
                    declared_type: Type::Float,
                    name: name.into(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                })
                .collect(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("numerator".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        function.statements = vec![
            Statement::Assign {
                name: "numerator".into(),
                value: Expression::FloatLiteral(2.0),
            },
            Statement::Assign {
                name: "denominator".into(),
                value: Expression::FloatLiteral(4.0),
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Variable("denominator".into())),
                    right: Box::new(Expression::FloatLiteral(0.0)),
                },
                then_body: vec![Statement::Assign {
                    name: "numerator".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Divide,
                        left: Box::new(Expression::Variable("numerator".into())),
                        right: Box::new(Expression::Variable("denominator".into())),
                    },
                }],
                else_body: Vec::new(),
            },
        ];

        assert!(summarize_automatic(&function).is_some());
    }
}
