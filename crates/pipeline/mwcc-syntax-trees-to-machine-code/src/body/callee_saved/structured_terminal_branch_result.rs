//! Promote a terminal if/else result local into direct branch returns.
//!
//! A scalar initialized only to satisfy C's definite-value rules and assigned
//! at the end of both arms has no independent lifetime. MWCC routes each arm's
//! value directly through the ABI result register and joins at the epilogue.
//! Normalizing that source identity before liveness keeps the generic branch
//! and return emitters responsible for the resulting control flow.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};

pub(super) fn fold(function: &Function) -> Option<Function> {
    let Expression::Variable(returned) = function.return_expression.as_ref()? else {
        return None;
    };
    let local_index = function.locals.iter().position(|local| {
        local.name == *returned
            && matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            && matches!(local.initializer, Some(Expression::IntegerLiteral(_)))
            && !local.is_volatile
    })?;
    let [Statement::If {
        then_body,
        else_body,
        ..
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let then_value = terminal_assignment(then_body, returned)?;
    let else_value = terminal_assignment(else_body, returned)?;
    if expression_reads_name(then_value, returned) || expression_reads_name(else_value, returned) {
        return None;
    }

    let mut rewritten = function.clone();
    let Statement::If {
        then_body,
        else_body,
        ..
    } = &mut rewritten.statements[0]
    else {
        unreachable!("the terminal branch was matched above")
    };
    replace_terminal_assignment(then_body, returned);
    replace_terminal_assignment(else_body, returned);
    rewritten.locals.remove(local_index);
    rewritten.return_expression = None;
    Some(rewritten)
}

fn terminal_assignment<'a>(statements: &'a [Statement], name: &str) -> Option<&'a Expression> {
    let (last, prefix) = statements.split_last()?;
    if prefix.iter().any(|statement| statement_reads_or_writes_name(statement, name)) {
        return None;
    }
    let Statement::Assign {
        name: assigned,
        value,
    } = last
    else {
        return None;
    };
    (assigned == name).then_some(value)
}

fn replace_terminal_assignment(statements: &mut [Statement], name: &str) {
    let Some(Statement::Assign {
        name: assigned,
        value,
    }) = statements.last()
    else {
        unreachable!("the terminal assignment was matched above")
    };
    assert_eq!(assigned, name);
    let value = value.clone();
    *statements.last_mut().expect("matched terminal statement") =
        Statement::Return(Some(value));
}

fn statement_reads_or_writes_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign { name: assigned, value } => {
            assigned == name || expression_reads_name(value, name)
        }
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            expression_reads_name(expression, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body
                    .iter()
                    .chain(else_body)
                    .any(|statement| statement_reads_or_writes_name(statement, name))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            [initializer, condition, step]
                .into_iter()
                .flatten()
                .any(|expression| expression_reads_name(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_reads_or_writes_name(statement, name))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms
                    .iter()
                    .any(|arm| arm_body_reads_or_writes_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_reads_or_writes_name(body, name))
        }
        Statement::Return(None)
        | Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn arm_body_reads_or_writes_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(expression) => expression_reads_name(expression, name),
        ArmBody::Statements(statements) => statements
            .iter()
            .any(|statement| statement_reads_or_writes_name(statement, name)),
    }
}

fn expression_reads_name(expression: &Expression, name: &str) -> bool {
    let mut found = false;
    super::structured_expression_visit::visit_expression(expression, &mut |expression| {
        found |= matches!(expression, Expression::Variable(candidate) if candidate == name);
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::LocalDeclaration;

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "result".into(),
                initializer: Some(Expression::IntegerLiteral(0)),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::Variable("flag".into()),
                then_body: vec![Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(7),
                }],
                else_body: vec![
                    Statement::Expression(Expression::Call {
                        name: "prepare".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Assign {
                        name: "result".into(),
                        value: Expression::Call {
                            name: "finish".into(),
                            arguments: Vec::new(),
                        },
                    },
                ],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
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
    fn promotes_both_terminal_assignments_to_returns() {
        let rewritten = fold(&function()).expect("terminal branch result");
        assert!(rewritten.locals.is_empty());
        assert!(rewritten.return_expression.is_none());
        let Statement::If {
            then_body,
            else_body,
            ..
        } = &rewritten.statements[0]
        else {
            unreachable!()
        };
        assert!(matches!(then_body.last(), Some(Statement::Return(Some(Expression::IntegerLiteral(7))))));
        assert!(matches!(else_body.last(), Some(Statement::Return(Some(Expression::Call { name, .. }))) if name == "finish"));
    }
}
