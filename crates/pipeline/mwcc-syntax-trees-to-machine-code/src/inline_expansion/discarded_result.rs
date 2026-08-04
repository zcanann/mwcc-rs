//! Elision of a discarded inline callee's write-only result local.
//!
//! A statement-valued inline body can retain a local used only to assemble its
//! source-level return value. When the caller discards that value, assignments
//! to the local are dead, but side effects in their right-hand sides are not.

use mwcc_syntax_trees::{ArmBody, BinaryOperator, Expression, Function, Statement, UnaryOperator};

use crate::analysis::{expression_has_side_effect, expression_reads_name};

pub(super) fn write_only_result_local(function: &Function) -> Option<&str> {
    let Expression::Variable(name) = function.return_expression.as_ref()? else {
        return None;
    };
    if !function.locals.iter().any(|local| local.name == *name)
        || function.locals.iter().any(|local| {
            local
                .initializer
                .as_ref()
                .is_some_and(|value| expression_reads_name(value, name))
        })
        || function.guards.iter().any(|guard| {
            expression_reads_name(&guard.condition, name)
                || expression_reads_name(&guard.value, name)
        })
        || statements_read_name(&function.statements, name)
    {
        return None;
    }
    Some(name)
}

/// Find a scalar accumulator used only to choose the callee's return value.
/// Once that value is discarded, its assignments may be reduced to their
/// side effects and its trailing guards may be omitted entirely.
pub(super) fn guarded_accumulator_local(function: &Function) -> Option<&str> {
    let first = function.guards.first()?;
    let Expression::Variable(name) = &first.condition else {
        return None;
    };
    if !function.locals.iter().any(|local| local.name == *name)
        || function.guards.iter().any(|guard| {
            !matches!(&guard.condition, Expression::Variable(guarded) if guarded == name)
                || expression_reads_name(&guard.value, name)
        })
        || function
            .return_expression
            .as_ref()
            .is_some_and(|value| expression_reads_name(value, name))
        || function.locals.iter().any(|local| {
            local
                .initializer
                .as_ref()
                .is_some_and(|value| expression_reads_name(value, name))
        })
        || !statements_only_accumulate_name(&function.statements, name)
    {
        return None;
    }
    Some(name)
}

pub(super) fn remove_assignments(statements: Vec<Statement>, result_name: &str) -> Vec<Statement> {
    statements
        .into_iter()
        .filter_map(|statement| remove_assignment(statement, result_name))
        .collect()
}

fn remove_assignment(statement: Statement, result_name: &str) -> Option<Statement> {
    match statement {
        Statement::Assign { name, value } if name == result_name => {
            discarded_accumulator_effect(&value, result_name)
                .or_else(|| expression_has_side_effect(&value).then_some(value))
                .map(Statement::Expression)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Some(Statement::If {
            condition,
            then_body: remove_assignments(then_body, result_name),
            else_body: remove_assignments(else_body, result_name),
        }),
        Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } => Some(Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body: remove_assignments(body, result_name),
        }),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => Some(Statement::Switch {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|mut arm| {
                    if let ArmBody::Statements(body) = arm.body {
                        arm.body = ArmBody::Statements(remove_assignments(body, result_name));
                    }
                    arm
                })
                .collect(),
            default: default.map(|body| match body {
                ArmBody::Statements(body) => {
                    ArmBody::Statements(remove_assignments(body, result_name))
                }
                returned @ ArmBody::Return(_) => returned,
            }),
        }),
        other => Some(other),
    }
}

fn discarded_accumulator_effect(value: &Expression, result_name: &str) -> Option<Expression> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = right.as_ref()
    else {
        return None;
    };
    matches!(left.as_ref(), Expression::Variable(read) if read == result_name)
        .then(|| match operand.as_ref() {
            call @ (Expression::Call { .. } | Expression::CallThrough { .. }) => {
                Some(call.clone())
            }
            _ => None,
        })
        .flatten()
}

fn statements_read_name(statements: &[Statement], name: &str) -> bool {
    statements
        .iter()
        .any(|statement| statement_reads_name(statement, name))
}

fn statements_only_accumulate_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign {
            name: target,
            value,
        } => target == name || !expression_reads_name(value, name),
        Statement::Store { target, value } => {
            !expression_reads_name(target, name) && !expression_reads_name(value, name)
        }
        Statement::Expression(value) => !expression_reads_name(value, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            !expression_reads_name(condition, name)
                && statements_only_accumulate_name(then_body, name)
                && statements_only_accumulate_name(else_body, name)
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
                .all(|value| !expression_reads_name(value, name))
                && statements_only_accumulate_name(body, name)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            !expression_reads_name(scrutinee, name)
                && arms.iter().all(|arm| match &arm.body {
                    ArmBody::Return(value) => !expression_reads_name(value, name),
                    ArmBody::Statements(body) => statements_only_accumulate_name(body, name),
                })
                && default.as_ref().is_none_or(|body| match body {
                    ArmBody::Return(value) => !expression_reads_name(value, name),
                    ArmBody::Statements(body) => statements_only_accumulate_name(body, name),
                })
        }
        Statement::Return(value) => value
            .as_ref()
            .is_none_or(|value| !expression_reads_name(value, name)),
        Statement::InlineAsm(_) => false,
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => true,
    })
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_reads_name(value, name)
        }
        Statement::InlineAsm(_) => true,
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || statements_read_name(then_body, name)
                || statements_read_name(else_body, name)
        }
        Statement::Return(value) => value
            .as_ref()
            .is_some_and(|value| expression_reads_name(value, name)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms.iter().any(|arm| arm_body_reads_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_reads_name(body, name))
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
                .any(|value| expression_reads_name(value, name))
                || statements_read_name(body, name)
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
    }
}

fn arm_body_reads_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(value) => expression_reads_name(value, name),
        ArmBody::Statements(statements) => statements_read_name(statements, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.to_owned())
    }

    #[test]
    fn removes_pure_assignments_but_preserves_rhs_calls() {
        let statements = vec![
            Statement::Assign {
                name: "result".to_owned(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: variable("condition"),
                then_body: vec![Statement::Assign {
                    name: "result".to_owned(),
                    value: Expression::Call {
                        name: "effect".to_owned(),
                        arguments: Vec::new(),
                    },
                }],
                else_body: Vec::new(),
            },
        ];

        let pruned = remove_assignments(statements, "result");
        assert!(matches!(
            pruned.as_slice(),
            [Statement::If { then_body, .. }]
                if matches!(
                    then_body.as_slice(),
                    [Statement::Expression(Expression::Call { name, .. })]
                        if name == "effect"
                )
        ));
    }

    #[test]
    fn detects_reads_in_nested_control_flow() {
        let statements = vec![Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition: Some(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(variable("result")),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
            step: None,
            body: Vec::new(),
        }];

        assert!(statements_read_name(&statements, "result"));
    }
}
