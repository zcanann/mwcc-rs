//! Source-home retention for unoptimized calls fed by an automatic aggregate.
//!
//! MWCC assigns distinct scalar-local homes to both the temporary address of a
//! frame aggregate and the result of the call that consumes it. Pure liveness
//! would forward the address in `r3` and leave the result there, but that loses
//! the O0 source-value graph and changes every later saved-register assignment.

use super::*;
use super::structured_locals::body_uses_local;

pub(super) struct StructuredUnoptimizedFrameCallHomes {
    names: std::collections::HashSet<String>,
    preferences: std::collections::HashMap<String, u8>,
}

impl StructuredUnoptimizedFrameCallHomes {
    pub(super) fn plan(function: &Function, frame_aggregates: &[&str]) -> Option<Self> {
        let [parameter] = function.parameters.as_slice() else {
            return None;
        };
        let (transaction_index, address_name, result_name) =
            framed_call_transaction(&function.statements, frame_aggregates)?;
        let general_locals: Vec<_> = function
            .locals
            .iter()
            .filter(|local| {
                !local.is_static
                    && !local.is_volatile
                    && local.array_length.is_none()
                    && local.initializer.is_none()
                    && is_source_general_scalar(local.declared_type)
                    && body_uses_local(&function.statements, &local.name)
            })
            .collect();
        if general_locals.len() != 5
            || !general_locals.iter().any(|local| local.name == address_name)
            || !general_locals.iter().any(|local| local.name == result_name)
        {
            return None;
        }

        let mut assignments: Vec<_> = general_locals
            .iter()
            .filter(|local| local.name != address_name && local.name != result_name)
            .map(|local| {
                function
                    .statements
                    .iter()
                    .position(|statement| {
                        matches!(statement, Statement::Assign { name, .. } if name == &local.name)
                    })
                    .map(|index| (index, local.name.as_str()))
            })
            .collect::<Option<Vec<_>>>()?;
        assignments.sort_unstable_by_key(|(index, _)| *index);
        let before: Vec<_> = assignments
            .iter()
            .filter(|(index, _)| *index < transaction_index)
            .map(|(_, name)| *name)
            .collect();
        let after: Vec<_> = assignments
            .iter()
            .filter(|(index, _)| *index > transaction_index + 1)
            .map(|(_, name)| *name)
            .collect();
        let ([entry_pointer], [first_post_call, second_post_call]) =
            (before.as_slice(), after.as_slice())
        else {
            return None;
        };
        let preferences = frame_call_role_preferences(
            &parameter.name,
            entry_pointer,
            &address_name,
            &result_name,
            first_post_call,
            second_post_call,
        );
        Some(Self {
            names: preferences.keys().cloned().collect(),
            preferences,
        })
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub(super) fn retains_distinct_parameter_home(&self) -> bool {
        true
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        self.preferences.get(name).copied()
    }
}

fn is_source_general_scalar(declared_type: Type) -> bool {
    !matches!(declared_type, Type::Struct { .. })
        && class_of(declared_type).ok() == Some(ValueClass::General)
}

#[cfg(test)]
fn framed_call_transaction_names(
    statements: &[Statement],
    frame_aggregates: &[&str],
) -> Vec<String> {
    framed_call_transaction(statements, frame_aggregates)
        .map(|(_, address, result)| vec![address, result])
        .unwrap_or_default()
}

fn framed_call_transaction(
    statements: &[Statement],
    frame_aggregates: &[&str],
) -> Option<(usize, String, String)> {
    for (index, window) in statements.windows(2).enumerate() {
        let [
            Statement::Assign {
                name: address_name,
                value:
                    Expression::AddressOf {
                        operand: addressed,
                    },
            },
            Statement::Assign {
                name: result_name,
                value: Expression::Call { arguments, .. },
            },
        ] = window
        else {
            continue;
        };
        let Expression::Variable(aggregate_name) = addressed.as_ref() else {
            continue;
        };
        if frame_aggregates.contains(&aggregate_name.as_str())
            && arguments.iter().any(
                |argument| matches!(argument, Expression::Variable(name) if name == address_name),
            )
        {
            return Some((index, address_name.clone(), result_name.clone()));
        }
    }
    None
}

fn frame_call_role_preferences(
    parameter: &str,
    entry_pointer: &str,
    address: &str,
    result: &str,
    first_post_call: &str,
    second_post_call: &str,
) -> std::collections::HashMap<String, u8> {
    [
        (parameter, 27),
        (entry_pointer, 30),
        (address, 26),
        (result, 28),
        (first_post_call, 31),
        (second_post_call, 29),
    ]
    .into_iter()
    .map(|(name, register)| (name.to_owned(), register))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        frame_call_role_preferences, framed_call_transaction_names, is_source_general_scalar,
    };
    use mwcc_syntax_trees::{Expression, Statement, Type};

    #[test]
    fn recognizes_an_address_local_and_its_call_result() {
        let statements = vec![
            Statement::Assign {
                name: "address".into(),
                value: Expression::AddressOf {
                    operand: Box::new(Expression::Variable("record".into())),
                },
            },
            Statement::Assign {
                name: "result".into(),
                value: Expression::Call {
                    name: "inspect".into(),
                    arguments: vec![Expression::Variable("address".into())],
                },
            },
        ];
        assert_eq!(
            framed_call_transaction_names(&statements, &["record"]),
            ["address", "result"],
        );
    }

    #[test]
    fn colors_unoptimized_frame_call_values_by_source_role() {
        let preferences = frame_call_role_preferences(
            "object",
            "entry_pointer",
            "address",
            "result",
            "first_post_call",
            "second_post_call",
        );
        assert_eq!(preferences["object"], 27);
        assert_eq!(preferences["entry_pointer"], 30);
        assert_eq!(preferences["address"], 26);
        assert_eq!(preferences["result"], 28);
        assert_eq!(preferences["first_post_call"], 31);
        assert_eq!(preferences["second_post_call"], 29);
    }

    #[test]
    fn excludes_frame_aggregates_from_scalar_roles() {
        assert!(!is_source_general_scalar(Type::Struct { size: 12, align: 4 }));
        assert!(is_source_general_scalar(Type::Int));
        assert!(is_source_general_scalar(Type::StructPointer { element_size: 12 }));
    }
}
