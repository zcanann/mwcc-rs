//! Strength reduction for counted file-scope pointer-table loops.
//!
//! MWCC carries both the source element index and its four-byte table offset.
//! Making that second induction value explicit before liveness planning lets
//! loads, stores, calls, and the register allocator share one representation.

use super::*;
use super::structured_expression_visit::visit_statement;

pub(super) fn strength_reduce_pointer_table_indices(
    function: &Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_name = 0usize;
    let mut declarations = Vec::new();
    let mut statements = Vec::with_capacity(function.statements.len());
    let mut changed = false;

    for (position, statement) in function.statements.iter().enumerate() {
        let previous = position
            .checked_sub(1)
            .and_then(|previous| function.statements.get(previous));
        let Some(index) = recognized_index(statement, previous, &function.locals, globals) else {
            statements.push(statement.clone());
            continue;
        };
        let local_initializer = function.locals.iter().any(|local| {
            local.name == index
                && local
                    .initializer
                    .as_ref()
                    .and_then(constant_value)
                    == Some(0)
        });
        let cursor = loop {
            let candidate = format!(
                "{}{}",
                crate::analysis::PRESCALED_POINTER_TABLE_INDEX_PREFIX,
                next_name
            );
            next_name += 1;
            if used.insert(candidate.clone()) {
                break candidate;
            }
        };
        declarations.push((
            index.clone(),
            LocalDeclaration {
                declared_type: Type::UnsignedInt,
                name: cursor.clone(),
                initializer: local_initializer.then(|| Expression::IntegerLiteral(0)),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            },
        ));
        if matches!(statement, Statement::Loop { initializer: None, .. })
            && !local_initializer
        {
            let previous = statements
                .pop()
                .expect("a detached pointer-table loop initializer was recognized");
            statements.push(zero_assignment_like(&previous, &cursor));
            statements.push(previous);
        }
        statements.push(rewrite_loop(statement, &index, &cursor));
        changed = true;
    }

    changed.then(|| {
        let mut reduced = function.clone();
        reduced.locals = function
            .locals
            .iter()
            .flat_map(|local| {
                declarations
                    .iter()
                    .filter(|(index, _)| index == &local.name)
                    .map(|(_, declaration)| declaration.clone())
                    .chain(std::iter::once(local.clone()))
            })
            .collect();
        reduced.statements = statements;
        reduced
    })
}

fn recognized_index(
    statement: &Statement,
    previous: Option<&Statement>,
    locals: &[LocalDeclaration],
    globals: &std::collections::HashMap<String, Type>,
) -> Option<String> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer,
        condition: Some(_),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    let index = unit_step_index(step)?;
    match initializer {
        Some(Expression::Assign { target, value })
            if matches!(target.as_ref(), Expression::Variable(name) if name == index)
                && constant_value(value) == Some(0) => {}
        None if previous.is_some_and(|statement| initializes_zero(statement, index))
            || locals.iter().any(|local| {
                local.name == index
                    && local
                        .initializer
                        .as_ref()
                        .and_then(constant_value)
                        == Some(0)
            }) => {}
        _ => return None,
    }

    let mut reads = 0usize;
    let mut pointer_indices = 0usize;
    for statement in body {
        visit_statement(statement, &mut |expression| match expression {
            Expression::Variable(name) if name == index => reads += 1,
            Expression::Index { base, index: used } => {
                let (Expression::Variable(global), Expression::Variable(used)) =
                    (base.as_ref(), used.as_ref())
                else {
                    return;
                };
                if used == index
                    && matches!(
                        globals.get(global),
                        Some(Type::Pointer(Pointee::Pointer | Pointee::WordPointer))
                    )
                {
                    pointer_indices += 1;
                }
            }
            _ => {}
        });
    }
    (pointer_indices != 0 && pointer_indices == reads).then(|| index.to_owned())
}

fn unit_step_index(step: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = step else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    matches!(
        value.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == index)
            && constant_value(right) == Some(1)
    )
    .then_some(index)
}

fn initializes_zero(statement: &Statement, index: &str) -> bool {
    matches!(
        statement,
        Statement::Assign { name, value }
            if name == index && constant_value(value) == Some(0)
    ) || matches!(
        statement,
        Statement::Expression(Expression::Assign { target, value })
            if matches!(target.as_ref(), Expression::Variable(name) if name == index)
                && constant_value(value) == Some(0)
    )
}

fn zero_assignment_like(statement: &Statement, name: &str) -> Statement {
    match statement {
        Statement::Assign { .. } => Statement::Assign {
            name: name.to_owned(),
            value: Expression::IntegerLiteral(0),
        },
        Statement::Expression(Expression::Assign { .. }) => {
            Statement::Expression(Expression::Assign {
                target: Box::new(Expression::Variable(name.to_owned())),
                value: Box::new(Expression::IntegerLiteral(0)),
            })
        }
        _ => unreachable!("detached zero initializer shape was checked"),
    }
}

fn rewrite_loop(statement: &Statement, index: &str, cursor: &str) -> Statement {
    let Statement::Loop {
        kind,
        initializer,
        condition,
        step,
        body,
    } = statement
    else {
        unreachable!("pointer-table cursor was recognized from a loop")
    };
    let values = std::collections::HashMap::from([(
        index.to_owned(),
        Expression::Variable(cursor.to_owned()),
    )]);
    Statement::Loop {
        kind: *kind,
        initializer: initializer.as_ref().map(|initializer| Expression::Comma {
            left: Box::new(initializer.clone()),
            right: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable(cursor.to_owned())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
        }),
        condition: condition.clone(),
        step: Some(Expression::Comma {
            left: Box::new(step.clone().expect("recognized step")),
            right: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable(cursor.to_owned())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable(cursor.to_owned())),
                    right: Box::new(Expression::IntegerLiteral(4)),
                }),
            }),
        }),
        body: body
            .iter()
            .map(|statement| substitute_statement(statement, &values))
            .collect(),
    }
}

pub(super) fn substitute_statement(
    statement: &Statement,
    values: &std::collections::HashMap<String, Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: crate::value_tracking::substitute(target, values),
            value: crate::value_tracking::substitute(value, values),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: crate::value_tracking::substitute(value, values),
        },
        Statement::Expression(value) => {
            Statement::Expression(crate::value_tracking::substitute(value, values))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: crate::value_tracking::substitute(condition, values),
            then_body: then_body
                .iter()
                .map(|statement| substitute_statement(statement, values))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| substitute_statement(statement, values))
                .collect(),
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_table_loop(extra: Vec<Statement>) -> Statement {
        let mut body = vec![Statement::Store {
            target: Expression::Index {
                base: Box::new(Expression::Variable("table".into())),
                index: Box::new(Expression::Variable("i".into())),
            },
            value: Expression::Call {
                name: "allocate".into(),
                arguments: Vec::new(),
            },
        }];
        body.extend(extra);
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::Variable("count".into())),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body,
        }
    }

    fn globals() -> std::collections::HashMap<String, Type> {
        std::collections::HashMap::from([(
            "table".into(),
            Type::Pointer(Pointee::Pointer),
        )])
    }

    #[test]
    fn recognizes_an_index_used_only_for_pointer_table_accesses() {
        assert_eq!(
            recognized_index(&pointer_table_loop(Vec::new()), None, &[], &globals()),
            Some("i".into())
        );
    }

    #[test]
    fn recognizes_an_immediately_preceding_zero_initializer() {
        let mut loop_statement = pointer_table_loop(Vec::new());
        let Statement::Loop { initializer, .. } = &mut loop_statement else {
            unreachable!()
        };
        *initializer = None;
        let previous = Statement::Assign {
            name: "i".into(),
            value: Expression::IntegerLiteral(0),
        };

        assert_eq!(
            recognized_index(&loop_statement, Some(&previous), &[], &globals()),
            Some("i".into())
        );
    }

    #[test]
    fn recognizes_a_local_declaration_zero_initializer() {
        let mut loop_statement = pointer_table_loop(Vec::new());
        let Statement::Loop { initializer, .. } = &mut loop_statement else {
            unreachable!()
        };
        *initializer = None;
        let local = LocalDeclaration {
            declared_type: Type::UnsignedInt,
            name: "i".into(),
            initializer: Some(Expression::IntegerLiteral(0)),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        };

        assert_eq!(
            recognized_index(&loop_statement, None, &[local], &globals()),
            Some("i".into())
        );
    }

    #[test]
    fn rejects_an_index_with_an_unscaled_body_use() {
        assert_eq!(
            recognized_index(
                &pointer_table_loop(vec![Statement::Expression(Expression::Variable(
                    "i".into(),
                ))]),
                None,
                &[],
                &globals(),
            ),
            None
        );
    }
}
