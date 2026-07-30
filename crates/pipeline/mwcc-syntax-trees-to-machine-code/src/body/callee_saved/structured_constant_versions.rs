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
}
