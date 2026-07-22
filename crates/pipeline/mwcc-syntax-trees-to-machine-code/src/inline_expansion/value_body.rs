//! Expression summaries for retained non-void inline functions.
//!
//! A call embedded in a condition cannot accept statement splicing without
//! changing short-circuit evaluation. This module recognizes the common
//! `result = A; if (condition) result = B; return result;` body and preserves
//! it as a comma/conditional expression at the original call position.

use mwcc_syntax_trees::{
    BinaryOperator, ConditionalOrigin, Expression, Function, Statement, Type, UnaryOperator,
};

#[derive(Clone, Debug)]
pub(super) struct ValueInlineBody {
    pub(super) source: Function,
    pub(super) expression: Expression,
}

pub(super) fn summarize(function: &Function) -> Option<ValueInlineBody> {
    if function.return_type == Type::Void
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    // A direct scalar/member return is the smallest value-inline body. Keep it
    // before the result-local pattern below: ordinary (non-inline) definitions
    // use this shape too, and mwcc's automatic inliner substitutes sufficiently
    // small accessors while still emitting their external definition.
    if function.locals.is_empty() && function.statements.is_empty() {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression: function.return_expression.clone()?,
        });
    }
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if result.is_static
        || result.is_volatile
        || result.array_length.is_some()
        || result.initializer.is_some()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        )
    {
        return None;
    }
    let (prefix, tail) = function
        .statements
        .split_at(function.statements.len().saturating_sub(2));
    if !prefix
        .iter()
        .all(|statement| matches!(statement, Statement::Expression(_)))
    {
        return None;
    }
    let [Statement::Assign {
        name: initial_name,
        value: Expression::IntegerLiteral(initial),
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }] = tail
    else {
        return None;
    };
    let [Statement::Assign {
        name: selected_name,
        value: Expression::IntegerLiteral(selected),
    }] = then_body.as_slice()
    else {
        return None;
    };
    if initial_name != &result.name || selected_name != &result.name || !else_body.is_empty() {
        return None;
    }

    let selection = if *initial == 0 && *selected == 1 && is_boolean_expression(condition) {
        condition.clone()
    } else {
        Expression::Conditional {
            condition: Box::new(condition.clone()),
            when_true: Box::new(Expression::IntegerLiteral(*selected)),
            when_false: Box::new(Expression::IntegerLiteral(*initial)),
            origin: ConditionalOrigin::IfAssignments,
        }
    };
    let expression = prefix.iter().rev().fold(selection, |right, statement| {
        let Statement::Expression(left) = statement else {
            unreachable!("prefix eligibility checked")
        };
        Expression::Comma {
            left: Box::new(left.clone()),
            right: Box::new(right),
        }
    });
    Some(ValueInlineBody {
        source: function.clone(),
        expression,
    })
}

/// Ordinary definitions are eligible for automatic value inlining only when
/// they are a direct expression body. More involved selection summaries remain
/// limited to definitions the frontend identified as explicitly/skipped inline.
pub(super) fn summarize_automatic(function: &Function) -> Option<ValueInlineBody> {
    if !function.locals.is_empty() || !function.statements.is_empty() {
        return None;
    }
    summarize(function)
}

fn is_boolean_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { operator, .. } => matches!(
            operator,
            BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual
                | BinaryOperator::LogicalAnd
                | BinaryOperator::LogicalOr
        ),
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            ..
        } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, LocalDeclaration, Parameter};

    fn empty_function(name: &str, return_type: Type) -> Function {
        Function {
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
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn summarizes_a_direct_member_accessor_for_automatic_inlining() {
        let mut function = empty_function("get", Type::Pointer(mwcc_syntax_trees::Pointee::Int));
        function.parameters.push(Parameter {
            parameter_type: Type::StructPointer { element_size: 16 },
            name: "object".into(),
        });
        function.return_expression = Some(Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 4,
            member_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
            index_stride: None,
        });

        let summary = summarize_automatic(&function).expect("direct accessor");
        assert!(matches!(summary.expression, Expression::Member { offset: 4, .. }));
    }

    #[test]
    fn summarizes_an_asserted_integer_selection() {
        let function = Function {
            return_type: Type::Int,
            name: "selected".into(),
            is_static: true,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "input".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "result".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Expression(Expression::Variable("assertion".into())),
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::If {
                    condition: Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(Expression::Variable("input".into())),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                    then_body: vec![Statement::Assign {
                        name: "result".into(),
                        value: Expression::IntegerLiteral(1),
                    }],
                    else_body: Vec::new(),
                },
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let summary = summarize(&function).expect("selection body should summarize");
        assert!(matches!(summary.expression, Expression::Comma { right, .. }
        if matches!(right.as_ref(), Expression::Binary {
            operator: BinaryOperator::NotEqual,
            ..
        })));
    }
}
