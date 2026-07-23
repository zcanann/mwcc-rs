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
        .find(|(name, _, incoming)| {
            (3..=10).contains(incoming) && expression_reads_name(first, name)
        })?;
    Some(EntryParameterAlias {
        name: name.clone(),
        home: *home,
        boundary: EntryAliasBoundary::AfterFirstConditionTerm,
    })
}

/// Fold the saved-home copy and an immediately following zero test into the
/// record form MWCC uses for an entry guard (`mr. r31,r3`). Keeping this beside
/// entry-alias planning prevents a general peephole from changing unrelated
/// copies whose CR0 side effect is not source-proven dead or consumed here.
pub(super) fn fold_entry_alias_zero_test(
    instructions: &mut Vec<Instruction>,
    alias: &EntryParameterAlias,
) -> bool {
    let [prefix @ .., compare] = instructions.as_slice() else {
        return false;
    };
    let Some((copy_index, incoming)) = prefix
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, instruction)| match instruction {
            Instruction::Or { a, s, b } if *a == alias.home && *s == *b => Some((index, *s)),
            _ => None,
        })
    else {
        return false;
    };
    // Moving the CR0 definition back across saved-register stores and plain
    // entry copies is safe; arithmetic, calls, and record instructions are not.
    if prefix[copy_index + 1..].iter().any(|instruction| {
        !matches!(
            instruction,
            Instruction::StoreWord { .. } | Instruction::Or { .. }
        )
    }) {
        return false;
    }
    let compares_incoming_to_zero = matches!(
        compare,
        Instruction::CompareWordImmediate { a, immediate: 0 }
            | Instruction::CompareLogicalWordImmediate { a, immediate: 0 }
            if *a == incoming
    );
    if !compares_incoming_to_zero {
        return false;
    }

    instructions[copy_index] = Instruction::OrRecord {
        a: alias.home,
        s: incoming,
        b: incoming,
    };
    instructions.pop();
    true
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

    #[test]
    fn preserves_a_second_parameter_entry_alias_through_the_first_guard_term() {
        let statements = vec![Statement::If {
            condition: Expression::Member {
                base: Box::new(Expression::Variable("object".to_string())),
                offset: 12,
                member_type: Type::Int,
                index_stride: None,
            },
            then_body: call(vec![Expression::Variable("object".to_string())]),
            else_body: vec![],
        }];
        let saved = vec![("object".to_string(), 31, 4)];

        let alias = plan_first_call_alias(&statements, &saved).expect("eligible alias");

        assert_eq!(alias.name, "object");
        assert_eq!(alias.home, 31);
        assert_eq!(alias.boundary, EntryAliasBoundary::AfterFirstConditionTerm);
    }

    #[test]
    fn folds_an_adjacent_entry_copy_and_zero_test() {
        let alias = EntryParameterAlias {
            name: "pointer".into(),
            home: 31,
            boundary: EntryAliasBoundary::AfterFirstConditionTerm,
        };
        let mut instructions = vec![
            Instruction::move_register(31, 3),
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        ];

        assert!(fold_entry_alias_zero_test(&mut instructions, &alias));
        assert!(matches!(
            instructions.as_slice(),
            [Instruction::OrRecord { a: 31, s: 3, b: 3 }]
        ));
    }

    #[test]
    fn folds_across_an_independent_saved_parameter_copy() {
        let alias = EntryParameterAlias {
            name: "pointer".into(),
            home: 31,
            boundary: EntryAliasBoundary::AfterFirstConditionTerm,
        };
        let mut instructions = vec![
            Instruction::move_register(31, 3),
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 8,
            },
            Instruction::move_register(30, 4),
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        ];

        assert!(fold_entry_alias_zero_test(&mut instructions, &alias));
        assert!(matches!(
            instructions.first(),
            Some(Instruction::OrRecord { a: 31, s: 3, b: 3 })
        ));
        assert_eq!(instructions.len(), 3);
    }
}
