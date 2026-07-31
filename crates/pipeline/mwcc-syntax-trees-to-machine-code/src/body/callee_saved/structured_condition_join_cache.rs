//! Condition values retained across a call-free `if` join.
//!
//! A value loaded for `if (current == prior) flag = 1;` dominates both paths
//! into a following guard.  Legacy MWCC keeps that value live when the guarded
//! body cannot call or overwrite it.  This module owns the source-level proof;
//! the structured emitter remains responsible for the cache lifetime.

use crate::condition_global_cache::ConditionGlobalValue;
use mwcc_syntax_trees::{Expression, Statement};
use std::collections::HashMap;

pub(super) fn followup_after_call_free_join<'a>(
    body: &[Statement],
    following: Option<&'a Statement>,
) -> Option<&'a Expression> {
    if !body.iter().all(is_direct_call_free_write) {
        return None;
    }
    let Some(Statement::If {
        condition,
        else_body,
        ..
    }) = following
    else {
        return None;
    };
    else_body.is_empty().then_some(condition)
}

pub(super) fn retained_values_after_join(
    mut values: HashMap<String, ConditionGlobalValue>,
    body: &[Statement],
) -> Option<HashMap<String, ConditionGlobalValue>> {
    for statement in body {
        let written = direct_written_name(statement)?;
        values.remove(written);
    }
    (!values.is_empty()).then_some(values)
}

fn is_direct_call_free_write(statement: &Statement) -> bool {
    direct_written_name(statement).is_some() && !crate::analysis::statement_has_call(statement)
}

fn direct_written_name(statement: &Statement) -> Option<&str> {
    match statement {
        Statement::Assign { name, .. } => Some(name),
        Statement::Store {
            target: Expression::Variable(name),
            ..
        } => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Statement {
        Statement::Store {
            target: Expression::Variable(name.into()),
            value: Expression::IntegerLiteral(1),
        }
    }

    fn guard() -> Statement {
        Statement::If {
            condition: Expression::Variable("current".into()),
            then_body: Vec::new(),
            else_body: Vec::new(),
        }
    }

    #[test]
    fn exposes_the_next_guard_after_a_direct_call_free_write() {
        assert!(followup_after_call_free_join(&[store("flag")], Some(&guard())).is_some());
    }

    #[test]
    fn removes_a_value_overwritten_in_the_guarded_body() {
        let values = HashMap::from([
            ("current".into(), ConditionGlobalValue::Register(3)),
            ("prior".into(), ConditionGlobalValue::Register(4)),
        ]);

        let retained = retained_values_after_join(values, &[store("current")])
            .expect("the untouched value remains reusable");

        assert!(!retained.contains_key("current"));
        assert!(retained.contains_key("prior"));
    }

    #[test]
    fn rejects_a_body_that_can_call() {
        let call = Statement::Assign {
            name: "flag".into(),
            value: Expression::Call {
                name: "refresh".into(),
                arguments: Vec::new(),
            },
        };

        assert!(followup_after_call_free_join(&[call], Some(&guard())).is_none());
    }
}
