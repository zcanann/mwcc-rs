//! Expression summaries for retained non-void inline functions.
//!
//! A call embedded in a condition cannot accept statement splicing without
//! changing short-circuit evaluation. This module recognizes the common
//! `result = A; if (condition) result = B; return result;` body and preserves
//! it as a comma/conditional expression at the original call position.

use super::safety::composable_function;
use mwcc_syntax_trees::{
    BinaryOperator, ConditionalOrigin, Expression, Function, Statement, Type, UnaryOperator,
};

#[derive(Clone, Debug)]
pub(super) struct ValueInlineBody {
    pub(super) source: Function,
    pub(super) expression: Expression,
}

impl ValueInlineBody {
    fn forwarded_call_arguments(&self) -> Option<&[Expression]> {
        match &self.expression {
            Expression::Call { arguments, .. } => Some(arguments),
            Expression::Comma { left, right }
                if matches!(right.as_ref(), Expression::IntegerLiteral(0)) =>
            {
                match left.as_ref() {
                    Expression::Call { arguments, .. } => Some(arguments),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Whether substituting caller arguments directly preserves both their
    /// single evaluation and source order. A pure forwarding wrapper uses
    /// every parameter exactly once, in its original position, so changing or
    /// side-effecting arguments do not need hygienic temporaries.
    pub(super) fn arguments_forwarded_once_in_order(&self) -> bool {
        let Some(forwarded) = self.forwarded_call_arguments() else {
            return false;
        };
        forwarded.len() == self.source.parameters.len()
            && forwarded
                .iter()
                .zip(&self.source.parameters)
                .all(|(argument, parameter)| {
                    matches!(argument, Expression::Variable(name) if name == &parameter.name)
                })
    }

    /// A pure caller expression can be substituted directly when the wrapper
    /// is one call and consumes this parameter exactly once. With no wrapper
    /// side effect before that call, materializing a compiler-only temporary
    /// would preserve semantics but lose MWCC's forwarding schedule.
    pub(super) fn parameter_used_once_in_forwarded_call(&self, name: &str) -> bool {
        self.forwarded_call_arguments().is_some_and(|arguments| {
            arguments
                .iter()
                .map(|argument| super::safety::expression_use_count(argument, name))
                .sum::<usize>()
                == 1
        })
    }
}

pub(super) fn summarize(function: &Function) -> Option<ValueInlineBody> {
    if !function.guards.is_empty() || function.asm_body.is_some() {
        return None;
    }
    if function.return_type == Type::Void {
        if function.return_expression.is_some()
            || (!composable_function(function) && !sequenced_aggregate_void_body(function))
            || !function.statements.iter().all(void_expression_statement)
        {
            return None;
        }
        return summarize_sequenced_body(function, Expression::IntegerLiteral(0)).map(
            |expression| ValueInlineBody {
                source: function.clone(),
                expression,
            },
        );
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
    if let Some(expression) = summarize_result_selection(function) {
        return Some(ValueInlineBody {
            source: function.clone(),
            expression,
        });
    }
    summarize_sequenced_body(function, function.return_expression.clone()?).map(|expression| {
        ValueInlineBody {
            source: function.clone(),
            expression,
        }
    })
}

fn sequenced_aggregate_void_body(function: &Function) -> bool {
    let local_names = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    function.locals.iter().any(|local| {
        local.initializer.is_none() && matches!(local.declared_type, Type::Struct { .. })
    }) && function.locals.iter().all(|local| {
        !local.is_static && !local.is_volatile && local.array_length.is_none()
    }) && assignments_target_only_locals(&function.statements, &local_names)
}

fn assignments_target_only_locals(
    statements: &[Statement],
    local_names: &std::collections::HashSet<&str>,
) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            assignments_target_only_locals(then_body, local_names)
                && assignments_target_only_locals(else_body, local_names)
        }
        _ => true,
    })
}

fn summarize_result_selection(function: &Function) -> Option<Expression> {
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
    Some(expression)
}

/// Convert a scalar inline body into a comma expression. Caller-owned fresh
/// locals are allocated when this summary is substituted, so initializers and
/// side effects still execute exactly where the original call appeared.
fn summarize_sequenced_body(function: &Function, result: Expression) -> Option<Expression> {
    if function.locals.len() > 8
        || statement_count(&function.statements) > 12
        || function.locals.iter().any(|local| {
            local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let mut expressions = Vec::new();
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            expressions.push(Expression::Assign {
                target: Box::new(Expression::Variable(local.name.clone())),
                value: Box::new(initializer.clone()),
            });
        }
    }
    for statement in &function.statements {
        expressions.push(statement_expression(statement)?);
    }
    expressions.push(result);
    Some(sequence(expressions))
}

fn void_expression_statement(statement: &Statement) -> bool {
    match statement {
        Statement::Store { .. } | Statement::Assign { .. } => true,
        Statement::Expression(expression) => assignment_sequence(expression),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().all(void_expression_statement)
                && else_body.iter().all(void_expression_statement)
        }
        _ => false,
    }
}

fn assignment_sequence(expression: &Expression) -> bool {
    match expression {
        Expression::Assign { .. } => true,
        Expression::Comma { left, right } => {
            assignment_sequence(left) && assignment_sequence(right)
        }
        _ => false,
    }
}

fn statement_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => 1 + statement_count(then_body) + statement_count(else_body),
            _ => 1,
        })
        .sum()
}

fn statement_expression(statement: &Statement) -> Option<Expression> {
    match statement {
        Statement::Expression(expression) => Some(expression.clone()),
        Statement::Assign { name, value } => Some(Expression::Assign {
            target: Box::new(Expression::Variable(name.clone())),
            value: Box::new(value.clone()),
        }),
        Statement::Store { target, value } => Some(Expression::Assign {
            target: Box::new(target.clone()),
            value: Box::new(value.clone()),
        }),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Some(Expression::Conditional {
            condition: Box::new(condition.clone()),
            when_true: Box::new(statement_sequence(then_body)?),
            when_false: Box::new(statement_sequence(else_body)?),
            origin: ConditionalOrigin::IfAssignments,
        }),
        Statement::Return(_)
        | Statement::Switch { .. }
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Loop { .. } => None,
    }
}

fn statement_sequence(statements: &[Statement]) -> Option<Expression> {
    let mut expressions = statements
        .iter()
        .map(statement_expression)
        .collect::<Option<Vec<_>>>()?;
    expressions.push(Expression::IntegerLiteral(0));
    Some(sequence(expressions))
}

fn sequence(expressions: Vec<Expression>) -> Expression {
    expressions
        .into_iter()
        .rev()
        .reduce(|right, left| Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        })
        .expect("a value-inline sequence always contains its return expression")
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

/// Ordinary one-use void wrappers are also automatic-inline candidates when
/// their entire body is one expression. Unlike statement-body composition,
/// the value representation can materialize changing caller arguments once at
/// the call site before substituting the wrapper, so branch-assigned values do
/// not prevent a semantics-preserving expansion.
pub(super) fn summarize_automatic_void_forward(function: &Function) -> Option<ValueInlineBody> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || function.return_expression.is_some()
        || !matches!(function.statements.as_slice(), [Statement::Expression(_)])
    {
        return None;
    }
    let [Statement::Expression(expression)] = function.statements.as_slice() else {
        unreachable!("single expression was checked")
    };
    Some(ValueInlineBody {
        source: function.clone(),
        expression: Expression::Comma {
            left: Box::new(expression.clone()),
            right: Box::new(Expression::IntegerLiteral(0)),
        },
    })
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
            inline_asm_blocks: Vec::new(),
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
        assert!(matches!(
            summary.expression,
            Expression::Member { offset: 4, .. }
        ));
    }

    #[test]
    fn recognizes_single_use_parameters_in_an_interleaved_forwarding_call() {
        let mut function = empty_function("forward", Type::Void);
        function.parameters = vec![
            Parameter {
                parameter_type: Type::StructPointer { element_size: 16 },
                name: "object".into(),
            },
            Parameter {
                parameter_type: Type::Float,
                name: "value".into(),
            },
        ];
        function.statements = vec![Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![
                Expression::Variable("object".into()),
                Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 4,
                    member_type: Type::Float,
                    index_stride: None,
                },
                Expression::Variable("value".into()),
            ],
        })];

        let summary = summarize_automatic_void_forward(&function).expect("void forwarder");
        assert!(!summary.arguments_forwarded_once_in_order());
        assert!(summary.parameter_used_once_in_forwarded_call("value"));
        assert!(!summary.parameter_used_once_in_forwarded_call("object"));
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
            inline_asm_blocks: Vec::new(),
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
