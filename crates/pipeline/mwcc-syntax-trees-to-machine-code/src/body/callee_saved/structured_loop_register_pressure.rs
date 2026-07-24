//! Dense saved-GPR windows for high-pressure structured loops.
//!
//! MWCC reserves the complete `r14..r31` window before allocating a loop whose
//! source value graph already spans at least that many scalar temporaries.  The
//! frame decision must happen before virtual-register allocation: discovering
//! the pressure later can grow an existing dense range, but cannot change an
//! individually saved frame into the helper-based contiguous form.

#[allow(unused_imports)]
use super::*;

use super::structured_locals::body_uses_local;

pub(super) const DENSE_SAVED_GPR_COUNT: usize = 18;

/// The first source local carried around a saturated loop.
///
/// MWCC assigns this primary loop quantum to r30. Later carried values require
/// coupled packet-cursor scheduling before their descending preferences are
/// safe, so this planner deliberately owns only the first role.
pub(super) fn primary_dense_loop_carried_local<'a>(
    statements: &[Statement],
    ephemeral_locals: &[&'a LocalDeclaration],
) -> Option<&'a str> {
    let loop_statement = dense_loop_statement(statements, ephemeral_locals)?;
    ephemeral_locals
        .iter()
        .filter(|local| class_of(local.declared_type).ok() == Some(ValueClass::General))
        .find(|local| loop_carries_name(loop_statement, &local.name))
        .map(|local| local.name.as_str())
}

/// Return the saved-home count for a source loop that saturates MWCC's saved
/// GPR window. Locals are counted per lexical loop, so unrelated temporaries in
/// separate loops cannot accidentally combine into a dense-frame signal.
pub(super) fn plan_dense_loop_register_window(
    statements: &[Statement],
    ephemeral_locals: &[&LocalDeclaration],
) -> Option<usize> {
    statements.iter().find_map(|statement| match statement {
        Statement::Loop { body, .. } => {
            let general_locals = ephemeral_locals
                .iter()
                .filter(|local| {
                    class_of(local.declared_type).ok() == Some(ValueClass::General)
                        && body_uses_local(std::slice::from_ref(statement), &local.name)
                })
                .count();
            (general_locals >= DENSE_SAVED_GPR_COUNT)
                .then_some(DENSE_SAVED_GPR_COUNT)
                .or_else(|| plan_dense_loop_register_window(body, ephemeral_locals))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => plan_dense_loop_register_window(then_body, ephemeral_locals)
            .or_else(|| plan_dense_loop_register_window(else_body, ephemeral_locals)),
        _ => None,
    })
}

fn dense_loop_statement<'a>(
    statements: &'a [Statement],
    ephemeral_locals: &[&LocalDeclaration],
) -> Option<&'a Statement> {
    statements.iter().find_map(|statement| match statement {
        Statement::Loop { body, .. } => {
            let general_locals = ephemeral_locals
                .iter()
                .filter(|local| {
                    class_of(local.declared_type).ok() == Some(ValueClass::General)
                        && body_uses_local(std::slice::from_ref(statement), &local.name)
                })
                .count();
            (general_locals >= DENSE_SAVED_GPR_COUNT)
                .then_some(statement)
                .or_else(|| dense_loop_statement(body, ephemeral_locals))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => dense_loop_statement(then_body, ephemeral_locals)
            .or_else(|| dense_loop_statement(else_body, ephemeral_locals)),
        _ => None,
    })
}

fn loop_carries_name(statement: &Statement, name: &str) -> bool {
    let Statement::Loop {
        condition,
        step,
        body,
        ..
    } = statement
    else {
        return false;
    };
    sequence_assigns_name(body, name)
        && (condition
            .as_ref()
            .is_some_and(|condition| expression_reads_name(condition, name))
            || reads_before_assignment(body, name)
            || step
                .as_ref()
                .is_some_and(|step| expression_reads_name(step, name)))
}

fn sequence_assigns_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => sequence_assigns_name(then_body, name) || sequence_assigns_name(else_body, name),
        Statement::Loop { body, .. } => sequence_assigns_name(body, name),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| arm_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_assigns_name(body, name))
        }
        _ => false,
    })
}

fn arm_assigns_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(_) => false,
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            sequence_assigns_name(statements, name)
        }
    }
}

fn reads_before_assignment(statements: &[Statement], name: &str) -> bool {
    for statement in statements {
        match statement {
            Statement::Assign {
                name: assigned,
                value,
            } if assigned == name => return expression_reads_name(value, name),
            _ if statement_references_name(statement, name) => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::UnsignedInt,
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

    fn read(name: &str) -> Statement {
        Statement::Expression(Expression::Variable(name.into()))
    }

    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.into(),
            value,
        }
    }

    #[test]
    fn reserves_the_complete_window_for_one_saturated_loop() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: locals.iter().map(|local| read(&local.name)).collect(),
        }];

        assert_eq!(
            plan_dense_loop_register_window(&statements, &references),
            Some(DENSE_SAVED_GPR_COUNT)
        );
    }

    #[test]
    fn does_not_combine_pressure_from_separate_loops() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: locals[..9].iter().map(|local| read(&local.name)).collect(),
            },
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: locals[9..].iter().map(|local| read(&local.name)).collect(),
            },
        ];

        assert_eq!(
            plan_dense_loop_register_window(&statements, &references),
            None
        );
    }

    #[test]
    fn selects_the_first_carried_value_in_a_saturated_loop() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let mut body = vec![
            read("v0"),
            assign(
                "v0",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v0".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
            assign("v1", Expression::IntegerLiteral(7)),
            read("v1"),
        ];
        body.extend(locals[2..].iter().map(|local| read(&local.name)));
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body,
        }];

        assert_eq!(
            primary_dense_loop_carried_local(&statements, &references),
            Some("v0")
        );
    }
}
