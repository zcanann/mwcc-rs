//! Source-home retention for unoptimized calls fed by an automatic aggregate.
//!
//! MWCC assigns distinct scalar-local homes to both the temporary address of a
//! frame aggregate and the result of the call that consumes it. Pure liveness
//! would forward the address in `r3` and leave the result there, but that loses
//! the O0 source-value graph and changes every later saved-register assignment.

use super::*;

pub(super) struct StructuredUnoptimizedFrameCallHomes {
    names: std::collections::HashSet<String>,
}

impl StructuredUnoptimizedFrameCallHomes {
    pub(super) fn plan(function: &Function, frame_aggregates: &[&str]) -> Option<Self> {
        let names = framed_call_transaction_names(&function.statements, frame_aggregates);
        if names.is_empty()
            || names.iter().any(|name| {
                function.locals.iter().find(|local| local.name == *name).is_none_or(
                    |local| {
                        local.is_static
                            || local.is_volatile
                            || local.array_length.is_some()
                            || class_of(local.declared_type).ok() != Some(ValueClass::General)
                    },
                )
            })
        {
            return None;
        }
        Some(Self {
            names: names.into_iter().collect(),
        })
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

fn framed_call_transaction_names(
    statements: &[Statement],
    frame_aggregates: &[&str],
) -> Vec<String> {
    for window in statements.windows(2) {
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
            return vec![address_name.clone(), result_name.clone()];
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::framed_call_transaction_names;
    use mwcc_syntax_trees::{Expression, Statement};

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
}
