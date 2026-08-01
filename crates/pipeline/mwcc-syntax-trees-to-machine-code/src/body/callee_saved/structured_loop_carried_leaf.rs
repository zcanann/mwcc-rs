//! Admission policy for frameless leaf CFGs with loop-carried local values.
//!
//! Specialized loop emitters retain first refusal. This recognizer only
//! identifies the residual topology that needs the general structured CFG
//! emitter: a source local is reassigned in a loop and its new value can feed a
//! later iteration or the continuation after the loop.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement};

pub(super) fn contains_loop_carried_local(function: &Function) -> bool {
    function
        .locals
        .iter()
        .any(|local| statements_carry_local(&function.statements, &local.name, false))
}

fn statements_carry_local(statements: &[Statement], name: &str, read_after: bool) -> bool {
    for (index, statement) in statements.iter().enumerate() {
        let continuation_reads = read_after
            || statements[index + 1..]
                .iter()
                .any(|statement| statement_reads_name(statement, name));
        match statement {
            Statement::Loop {
                condition,
                step,
                body,
                ..
            } => {
                let carried_read = continuation_reads
                    || condition
                        .as_ref()
                        .is_some_and(|condition| expression_reads_name(condition, name))
                    || step
                        .as_ref()
                        .is_some_and(|step| expression_reads_name(step, name))
                    || body.iter().any(|statement| statement_reads_name(statement, name));
                if carried_read && statements_assign_name(body, name) {
                    return true;
                }
                if statements_carry_local(body, name, continuation_reads) {
                    return true;
                }
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                if statements_carry_local(then_body, name, continuation_reads)
                    || statements_carry_local(else_body, name, continuation_reads)
                {
                    return true;
                }
            }
            Statement::Switch { arms, default, .. } => {
                if arms.iter().any(|arm| match &arm.body {
                    ArmBody::Statements(body) => {
                        statements_carry_local(body, name, continuation_reads)
                    }
                    ArmBody::Return(_) => false,
                }) || default.as_ref().is_some_and(|body| match body {
                    ArmBody::Statements(body) => {
                        statements_carry_local(body, name, continuation_reads)
                    }
                    ArmBody::Return(_) => false,
                }) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn statements_assign_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::Expression(Expression::Assign { target, .. }) => {
            matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name)
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statements_assign_name(then_body, name) || statements_assign_name(else_body, name)
        }
        Statement::Loop { body, .. } => statements_assign_name(body, name),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| match &arm.body {
                ArmBody::Statements(body) => statements_assign_name(body, name),
                ArmBody::Return(_) => false,
            }) || default.as_ref().is_some_and(|body| match body {
                ArmBody::Statements(body) => statements_assign_name(body, name),
                ArmBody::Return(_) => false,
            })
        }
        _ => false,
    })
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            expression_reads_name(expression, name)
        }
        Statement::Assign { value, .. } => expression_reads_name(value, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
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
                .is_some_and(|expression| expression_reads_name(expression, name))
                || condition
                    .as_ref()
                    .is_some_and(|expression| expression_reads_name(expression, name))
                || step
                    .as_ref()
                    .is_some_and(|expression| expression_reads_name(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms.iter().any(|arm| match &arm.body {
                    ArmBody::Statements(body) => body
                        .iter()
                        .any(|statement| statement_reads_name(statement, name)),
                    ArmBody::Return(value) => expression_reads_name(value, name),
                })
                || default.as_ref().is_some_and(|body| match body {
                    ArmBody::Statements(body) => body
                        .iter()
                        .any(|statement| statement_reads_name(statement, name)),
                    ArmBody::Return(value) => expression_reads_name(value, name),
                })
        }
        Statement::InlineAsm(_)
        | Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn expression_reads_name(expression: &Expression, name: &str) -> bool {
    crate::analysis::expression_reads_name(expression, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, LoopKind, Type};

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::StructPointer { element_size: 32 },
            name: "walk".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::StructPointer { element_size: 32 },
                name: "cursor".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements,
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("cursor".into())),
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
    fn recognizes_pointer_reassignment_read_by_the_next_condition_and_return() {
        let function = function(vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Variable("cursor".into())),
            step: None,
            body: vec![Statement::Assign {
                name: "cursor".into(),
                value: Expression::Member {
                    base: Box::new(Expression::Variable("cursor".into())),
                    offset: 8,
                    member_type: Type::StructPointer { element_size: 32 },
                    index_stride: None,
                },
            }],
        }]);

        assert!(contains_loop_carried_local(&function));
    }

    #[test]
    fn ignores_loop_local_assignment_without_a_carried_or_continuation_read() {
        let mut function = function(vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Variable("keep_going".into())),
            step: None,
            body: vec![Statement::Assign {
                name: "cursor".into(),
                value: Expression::Variable("head".into()),
            }],
        }]);
        function.return_expression = Some(Expression::IntegerLiteral(0));

        assert!(!contains_loop_carried_local(&function));
    }
}
