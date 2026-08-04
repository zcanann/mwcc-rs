//! Provenance for deferred saved homes that retain an optimizer frame lane.
//!
//! A physical saved register does not reveal whether its source value existed
//! at function entry or was created on a guarded path or after a call. Build
//! 163 retains one optimizer lane for those deferred families, even though the
//! value itself never spills. Keep that fact separate from frame reconciliation.

use mwcc_syntax_trees::{LocalDeclaration, Statement, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DeferredSavedLocalLane {
    pub(super) distinct_from_entry_parameter_table: bool,
}

pub(super) fn deferred_saved_local_lane(
    statements: &[Statement],
    saved_locals: &[&LocalDeclaration],
) -> Option<DeferredSavedLocalLane> {
    let [local] = saved_locals else {
        return None;
    };
    if local.initializer.is_some() {
        return None;
    }
    let pointer = matches!(
        local.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    );
    (block_assigns_inside_guard(statements, &local.name, false)
        || (pointer && assignment_follows_call(statements, &local.name)))
    .then_some(DeferredSavedLocalLane {
        // Scalar guarded results share build 163's ordinary inferred value
        // lane. Deferred pointer identities retain a separate optimizer table
        // when an entry parameter owns a saved home at the same time.
        distinct_from_entry_parameter_table: pointer,
    })
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

        assert_eq!(
            deferred_saved_local_lane(&statements, &[&local]),
            Some(DeferredSavedLocalLane {
                distinct_from_entry_parameter_table: true,
            })
        );
    }

    #[test]
    fn excludes_an_unguarded_assignment() {
        let local = deferred("finished");

        assert!(deferred_saved_local_lane(
            &[assignment("finished")],
            &[&local]
        )
        .is_none());
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

        assert_eq!(
            deferred_saved_local_lane(&statements, &[&local]),
            Some(DeferredSavedLocalLane {
                distinct_from_entry_parameter_table: true,
            })
        );
    }

    #[test]
    fn guarded_scalar_lane_is_shared_with_the_entry_table() {
        let mut local = deferred("result");
        local.declared_type = Type::Int;
        let statements = vec![Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![assignment("result")],
            else_body: vec![assignment("result")],
        }];

        assert_eq!(
            deferred_saved_local_lane(&statements, &[&local]),
            Some(DeferredSavedLocalLane {
                distinct_from_entry_parameter_table: false,
            })
        );
    }
}
