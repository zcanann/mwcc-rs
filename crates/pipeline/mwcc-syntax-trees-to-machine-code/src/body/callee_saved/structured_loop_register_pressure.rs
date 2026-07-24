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
}
