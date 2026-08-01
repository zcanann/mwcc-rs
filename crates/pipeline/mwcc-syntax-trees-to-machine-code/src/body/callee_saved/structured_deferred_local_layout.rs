//! Provenance for deferred saved homes that retain an optimizer frame lane.
//!
//! A physical saved register does not reveal whether its source value existed
//! at function entry or was created on a guarded path or after a call. Build
//! 163 retains one optimizer lane for those deferred families, even though the
//! value itself never spills. Keep that fact separate from frame reconciliation.

use mwcc_syntax_trees::{LocalDeclaration, Statement, Type};

pub(super) fn retains_deferred_saved_local_lane(
    statements: &[Statement],
    saved_locals: &[&LocalDeclaration],
) -> bool {
    saved_locals.len() == 1
        && saved_locals[0].initializer.is_none()
        && (block_assigns_inside_guard(statements, &saved_locals[0].name, false)
            || (matches!(
                saved_locals[0].declared_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            ) && assignment_follows_call(statements, &saved_locals[0].name)))
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

fn assignment_follows_call(statements: &[Statement], name: &str) -> bool {
    let mut call_seen = false;
    for statement in statements {
        if call_seen
            && matches!(
                statement,
                Statement::Assign {
                    name: assigned,
                    ..
                } if assigned == name
            )
        {
            return true;
        }
        call_seen |= crate::analysis::statement_has_call(statement);
    }
    false
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
            attribute_alignment: None,
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

        assert!(retains_deferred_saved_local_lane(&statements, &[&local]));
    }

    #[test]
    fn excludes_an_unguarded_assignment() {
        let local = deferred("finished");

        assert!(!retains_deferred_saved_local_lane(
            &[assignment("finished")],
            &[&local]
        ));
    }

    #[test]
    fn recognizes_a_deferred_saved_local_created_after_a_call() {
        let local = deferred("finished");
        let statements = vec![
            Statement::Expression(Expression::Call {
                name: "prepare".into(),
                arguments: Vec::new(),
            }),
            assignment("finished"),
        ];

        assert!(retains_deferred_saved_local_lane(&statements, &[&local]));
    }
}
