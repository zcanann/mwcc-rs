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
    if is_guarded_interpolation(function) {
        return value_body::summarize_bounded_sequenced_automatic(function, 16);
    }
    is_scalar_projection(function)
        .then(|| value_body::summarize(function))
        .flatten()
}

fn is_guarded_interpolation(function: &Function) -> bool {
    let [first, second, output, amount] = function.parameters.as_slice() else {
        return false;
    };
    let Some(output_mask) =
        interpolation_output_mask(&function.statements, &output.name, &amount.name)
    else {
        return false;
    };
    is_three_float_struct(first.parameter_type)
        && is_three_float_struct(second.parameter_type)
        && matches!(output.parameter_type, Type::StructPointer { element_size: 12 })
        && amount.parameter_type == Type::Float
        && function.locals.is_empty()
        && matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &amount.name)
        && matches!(function.statements.as_slice(), [Statement::If { .. }])
        && output_mask == 0b111
}

/// Return the output members definitely written on every path. Sequential
/// statements accumulate writes, while a conditional retains only writes made
/// by both arms.
fn interpolation_output_mask(
    statements: &[Statement],
    output_name: &str,
    amount_name: &str,
) -> Option<u8> {
    statements.iter().try_fold(0u8, |output_mask, statement| {
        let written = match statement {
            Statement::Assign { name, value }
                if name == amount_name && !crate::analysis::expression_has_side_effect(value) =>
            {
                0
            }
            Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type: Type::Float,
                        index_stride: None,
                    },
                value,
            } if matches!(base.as_ref(), Expression::Variable(name) if name == output_name)
                && !crate::analysis::expression_has_side_effect(value) =>
            {
                match offset {
                    0 => 0b001,
                    4 => 0b010,
                    8 => 0b100,
                    _ => return None,
                }
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } if !crate::analysis::expression_has_side_effect(condition) => {
                interpolation_output_mask(then_body, output_name, amount_name)?
                    & interpolation_output_mask(else_body, output_name, amount_name)?
            }
            _ => return None,
        };
        Some(output_mask | written)
    })
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

    #[test]
    fn summarizes_a_bounded_guarded_vector_interpolation() {
        let vector = Type::Struct { size: 12, align: 4 };
        let parameters = vec![
            Parameter {
                parameter_type: vector,
                name: "first".into(),
            },
            Parameter {
                parameter_type: vector,
                name: "second".into(),
            },
            Parameter {
                parameter_type: Type::StructPointer { element_size: 12 },
                name: "output".into(),
            },
            Parameter {
                parameter_type: Type::Float,
                name: "amount".into(),
            },
        ];
        let store = |offset| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("output".into())),
                offset,
                member_type: Type::Float,
                index_stride: None,
            },
            value: Expression::FloatLiteral(offset as f64),
        };
        let branch = |amount, stores: Vec<Statement>| {
            let mut body = stores;
            if let Some(value) = amount {
                body.push(Statement::Assign {
                    name: "amount".into(),
                    value: Expression::FloatLiteral(value),
                });
            }
            body
        };
        let function = Function {
            return_type: Type::Float,
            name: "interpolate".into(),
            is_static: false,
            is_weak: false,
            parameters,
            locals: Vec::new(),
            statements: vec![Statement::If {
                condition: Expression::FloatLiteral(1.0),
                then_body: branch(Some(0.0), vec![store(0), store(4), store(8)]),
                else_body: vec![Statement::If {
                    condition: Expression::FloatLiteral(1.0),
                    then_body: branch(Some(1.0), vec![store(0), store(4), store(8)]),
                    else_body: branch(None, vec![store(0), store(4), store(8)]),
                }],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("amount".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert!(is_guarded_interpolation(&function));
        assert!(value_body::summarize(&function).is_none());
        assert!(summarize_automatic(&function).is_some());
    }
}
