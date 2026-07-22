//! Entry-register aliases that remain valid through a structured body's first call.

#[allow(unused_imports)]
use super::*;
use super::structured::logical_and_terms;

#[derive(Clone)]
pub(super) struct EntryParameterAlias {
    pub(super) name: String,
    pub(super) home: u8,
    pub(super) boundary: EntryAliasBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryAliasBoundary {
    AfterFirstStatement,
    AfterFirstConditionTerm,
}

/// Identify a saved parameter that MWCC can forward to the first direct call
/// from its untouched incoming ABI register. Later uses switch to the saved
/// home after that call has clobbered the entry alias.
pub(super) fn plan_first_call_alias(
    statements: &[Statement],
    saved_parameters: &[(String, u8, u8)],
) -> Option<EntryParameterAlias> {
    if let Statement::Expression(Expression::Call { arguments, .. }) = statements.first()? {
        let (Expression::Variable(name), later_arguments) = arguments.split_first()? else {
            return None;
        };
        if later_arguments
            .iter()
            .any(|argument| expression_reads_name(argument, name))
        {
            return None;
        }
        let (_, home, _) = saved_parameters
            .iter()
            .find(|(saved_name, _, incoming)| saved_name == name && *incoming == 3)?;
        return Some(EntryParameterAlias {
            name: name.clone(),
            home: *home,
            boundary: EntryAliasBoundary::AfterFirstStatement,
        });
    }

    let Statement::If {
        condition,
        else_body,
        ..
    } = statements.first()?
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let terms = logical_and_terms(condition);
    let [first, ..] = terms.as_slice() else {
        return None;
    };
    let (name, home, _) = saved_parameters
        .iter()
        .find(|(name, _, incoming)| *incoming == 3 && expression_reads_name(first, name))?;
    Some(EntryParameterAlias {
        name: name.clone(),
        home: *home,
        boundary: EntryAliasBoundary::AfterFirstConditionTerm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(arguments: Vec<Expression>) -> Vec<Statement> {
        vec![Statement::Expression(Expression::Call {
            name: "sink".to_string(),
            arguments,
        })]
    }

    #[test]
    fn forwards_an_untouched_first_gpr_argument() {
        let statements = call(vec![
            Expression::Variable("pointer".to_string()),
            Expression::IntegerLiteral(7),
        ]);
        let saved = vec![("pointer".to_string(), 31, 3)];

        let alias = plan_first_call_alias(&statements, &saved).expect("eligible alias");

        assert_eq!(alias.name, "pointer");
        assert_eq!(alias.home, 31);
        assert_eq!(alias.boundary, EntryAliasBoundary::AfterFirstStatement);
    }

    #[test]
    fn rejects_a_parameter_reused_in_another_argument_slot() {
        let statements = call(vec![
            Expression::Variable("pointer".to_string()),
            Expression::Variable("pointer".to_string()),
        ]);
        let saved = vec![("pointer".to_string(), 31, 3)];

        assert!(plan_first_call_alias(&statements, &saved).is_none());
    }

    #[test]
    fn preserves_an_entry_alias_through_the_first_guard_term() {
        let statements = vec![Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("pointer".to_string())),
                    offset: 0,
                    member_type: Type::Int,
                    index_stride: None,
                }),
                right: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("pointer".to_string())),
                    offset: 4,
                    member_type: Type::Int,
                    index_stride: None,
                }),
            },
            then_body: call(vec![Expression::Variable("pointer".to_string())]),
            else_body: vec![],
        }];
        let saved = vec![("pointer".to_string(), 31, 3)];

        let alias = plan_first_call_alias(&statements, &saved).expect("eligible alias");

        assert_eq!(alias.boundary, EntryAliasBoundary::AfterFirstConditionTerm);
    }
}
