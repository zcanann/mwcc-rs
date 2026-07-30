//! Internal value versions for constants retained across structured calls.
//!
//! MWCC can promote two store immediates in one structured arm to a single
//! call-crossing value. Express that lifetime in the semantic body before
//! structured liveness runs, allowing the ordinary allocator to select and
//! save the required callee-saved register.

use super::*;

pub(super) fn retain_repeated_store_constant_across_call(function: &Function) -> Option<Function> {
    let occupied = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .collect::<std::collections::HashSet<_>>();
    let mut ordinal = 0;
    let name = loop {
        let candidate = format!("__mwcc_retained_constant_{ordinal}");
        ordinal += 1;
        if !occupied.contains(candidate.as_str()) {
            break candidate;
        }
    };

    let mut rewritten = function.clone();
    let constant = rewrite_statement_list(&mut rewritten.statements, &name)?;
    rewritten.locals.push(LocalDeclaration {
        declared_type: Type::Int,
        name,
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    });
    debug_assert!(i32::try_from(constant).is_ok());
    Some(rewritten)
}

fn rewrite_statement_list(statements: &mut Vec<Statement>, name: &str) -> Option<i64> {
    for statement in statements.iter_mut() {
        let nested = match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => rewrite_statement_list(then_body, name)
                .or_else(|| rewrite_statement_list(else_body, name)),
            Statement::Loop { body, .. } => rewrite_statement_list(body, name),
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
    }

    for first in 0..statements.len() {
        let Some(constant) = store_integer_constant(&statements[first]) else {
            continue;
        };
        if i32::try_from(constant).is_err() {
            continue;
        }
        if rewrite_guarded_second_store(&mut statements[first + 1..], constant, name, false) {
            let Statement::Store {
                value: first_value, ..
            } = &mut statements[first]
            else {
                unreachable!("the first guarded constant was classified as a store")
            };
            *first_value = Expression::Variable(name.to_owned());
            statements.insert(
                first,
                Statement::Assign {
                    name: name.to_owned(),
                    value: Expression::IntegerLiteral(constant),
                },
            );
            return Some(constant);
        }
        for second in first + 1..statements.len() {
            if store_integer_constant(&statements[second]) != Some(constant)
                || !statements[first + 1..second]
                    .iter()
                    .any(crate::analysis::statement_has_call)
            {
                continue;
            }
            let replacement = Expression::Variable(name.to_owned());
            let Statement::Store {
                value: first_value, ..
            } = &mut statements[first]
            else {
                unreachable!("the first repeated constant was classified as a store")
            };
            *first_value = replacement.clone();
            let Statement::Store {
                value: second_value,
                ..
            } = &mut statements[second]
            else {
                unreachable!("the second repeated constant was classified as a store")
            };
            *second_value = replacement;
            statements.insert(
                first,
                Statement::Assign {
                    name: name.to_owned(),
                    value: Expression::IntegerLiteral(constant),
                },
            );
            return Some(constant);
        }
    }
    None
}

fn rewrite_guarded_second_store(
    statements: &mut [Statement],
    constant: i64,
    name: &str,
    mut crossed_call: bool,
) -> bool {
    for statement in statements {
        if crossed_call && store_integer_constant(statement) == Some(constant) {
            let Statement::Store { value, .. } = statement else {
                unreachable!("the guarded constant was classified as a store")
            };
            *value = Expression::Variable(name.to_owned());
            return true;
        }
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let branch_crossed =
                    crossed_call || crate::analysis::expression_has_call(condition);
                if rewrite_guarded_second_store(then_body, constant, name, branch_crossed)
                    || rewrite_guarded_second_store(else_body, constant, name, branch_crossed)
                {
                    return true;
                }
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                let loop_crossed = crossed_call
                    || initializer
                        .as_ref()
                        .is_some_and(crate::analysis::expression_has_call)
                    || condition
                        .as_ref()
                        .is_some_and(crate::analysis::expression_has_call)
                    || step
                        .as_ref()
                        .is_some_and(crate::analysis::expression_has_call);
                if rewrite_guarded_second_store(body, constant, name, loop_crossed) {
                    return true;
                }
            }
            Statement::Switch { scrutinee, .. } => {
                crossed_call |= crate::analysis::expression_has_call(scrutinee);
            }
            _ => {
                crossed_call |= crate::analysis::statement_has_call(statement);
            }
        }
    }
    false
}

fn store_integer_constant(statement: &Statement) -> Option<i64> {
    let Statement::Store { value, .. } = statement else {
        return None;
    };
    constant_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_two_store_constants_separated_by_a_call() {
        let mut statements = vec![
            Statement::Store {
                target: Expression::Variable("first".into()),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Expression(Expression::Call {
                name: "initialize".into(),
                arguments: Vec::new(),
            }),
            Statement::Store {
                target: Expression::Variable("second".into()),
                value: Expression::IntegerLiteral(0),
            },
        ];

        assert_eq!(
            rewrite_statement_list(&mut statements, "__retained"),
            Some(0)
        );
        assert!(matches!(
            statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Store {
                    value: Expression::Variable(first),
                    ..
                },
                Statement::Expression(Expression::Call { .. }),
                Statement::Store {
                    value: Expression::Variable(second),
                    ..
                },
            ] if name == "__retained" && first == name && second == name
        ));
    }

    #[test]
    fn versions_a_constant_reused_in_a_guarded_tail_after_calls() {
        let mut statements = vec![
            Statement::Store {
                target: Expression::Variable("initialized".into()),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Expression(Expression::Call {
                name: "initialize".into(),
                arguments: Vec::new(),
            }),
            Statement::If {
                condition: Expression::Variable("bootrom".into()),
                then_body: Vec::new(),
                else_body: vec![Statement::Store {
                    target: Expression::Variable("first_time".into()),
                    value: Expression::IntegerLiteral(1),
                }],
            },
        ];

        assert_eq!(
            rewrite_statement_list(&mut statements, "__retained"),
            Some(1)
        );
        assert!(matches!(
            statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Store {
                    value: Expression::Variable(first),
                    ..
                },
                Statement::Expression(Expression::Call { .. }),
                Statement::If { else_body, .. },
            ] if name == "__retained"
                && first == name
                && matches!(
                    else_body.as_slice(),
                    [Statement::Store {
                        value: Expression::Variable(second),
                        ..
                    }] if second == name
                )
        ));
    }
}
