//! Provenance for deferred saved homes first materialized inside a guard.
//!
//! A physical saved register does not reveal whether its source value existed
//! at function entry or was created only on a guarded path. Build 163 retains
//! one optimizer lane for the latter family, even though the value itself never
//! spills. Keep that source-level fact separate from frame reconciliation.

use mwcc_syntax_trees::{LocalDeclaration, Statement};

pub(super) fn has_guarded_deferred_saved_local(
    statements: &[Statement],
    saved_locals: &[&LocalDeclaration],
) -> bool {
    saved_locals.len() == 1
        && saved_locals[0].initializer.is_none()
        && block_assigns_inside_guard(statements, &saved_locals[0].name, false)
}

fn block_assigns_inside_guard(statements: &[Statement], name: &str, guarded: bool) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => guarded && assigned == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            block_assigns_inside_guard(then_body, name, true)
                || block_assigns_inside_guard(else_body, name, true)
        }
        Statement::Loop { body, .. } => block_assigns_inside_guard(body, name, guarded),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Expression, Type};

    fn deferred(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 48 },
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

    fn assignment(name: &str) -> Statement {
        Statement::Assign {
            name: name.into(),
            value: Expression::Variable("source".into()),
        }
    }

    #[test]
    fn recognizes_a_deferred_saved_local_created_in_a_guard() {
        let local = deferred("finished");
        let statements = vec![Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![assignment("finished")],
            else_body: Vec::new(),
        }];

        assert!(has_guarded_deferred_saved_local(&statements, &[&local]));
    }

    #[test]
    fn excludes_an_unguarded_assignment() {
        let local = deferred("finished");

        assert!(!has_guarded_deferred_saved_local(
            &[assignment("finished")],
            &[&local]
        ));
    }
}
