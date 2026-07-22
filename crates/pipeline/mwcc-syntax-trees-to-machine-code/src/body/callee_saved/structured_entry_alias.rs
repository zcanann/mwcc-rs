//! Entry-register aliases that remain valid through a structured body's first call.

#[allow(unused_imports)]
use super::*;

pub(super) struct EntryParameterAlias {
    pub(super) name: String,
    pub(super) home: u8,
}

/// Identify a saved parameter that MWCC can forward to the first direct call
/// from its untouched incoming ABI register. Later uses switch to the saved
/// home after that call has clobbered the entry alias.
pub(super) fn plan_first_call_alias(
    statements: &[Statement],
    saved_parameters: &[(String, u8, u8)],
) -> Option<EntryParameterAlias> {
    let Statement::Expression(Expression::Call { arguments, .. }) = statements.first()? else {
        return None;
    };
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
    Some(EntryParameterAlias {
        name: name.clone(),
        home: *home,
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
}
