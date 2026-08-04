//! Internal value versions for constants retained across structured calls.
//!
//! MWCC can promote two store immediates in one structured arm to a single
//! call-crossing value. Express that lifetime in the semantic body before
//! structured liveness runs, allowing the ordinary allocator to select and
//! save the required callee-saved register.

use super::*;

const MAX_RETAINED_CONSTANT_HOME_PRESSURE: usize = 4;

fn retained_constant_home_capacity(use_lmw_stmw: bool) -> usize {
    MAX_RETAINED_CONSTANT_HOME_PRESSURE + usize::from(use_lmw_stmw)
}

pub(super) fn repeated_store_constant_exceeds_home_capacity(
    function: &Function,
    use_lmw_stmw: bool,
) -> bool {
    if existing_call_live_value_count(function) < retained_constant_home_capacity(use_lmw_stmw) {
        return false;
    }
    let mut statements = function.statements.clone();
    rewrite_statement_list(&mut statements, "__mwcc_retained_constant_probe").is_some()
}

pub(super) fn retain_repeated_store_constant_across_call(
    function: &Function,
    use_lmw_stmw: bool,
) -> Option<Function> {
    if existing_call_live_value_count(function) >= retained_constant_home_capacity(use_lmw_stmw) {
        return None;
    }
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
        attribute_alignment: None,
        row_bytes: None,
    });
    debug_assert!(i32::try_from(constant).is_ok());
    Some(rewritten)
}

fn existing_call_live_value_count(function: &Function) -> usize {
    function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .chain(function.parameters.iter().map(|parameter| parameter.name.as_str()))
        .filter(|name| {
            super::structured_liveness::read_after_possible_call_in_function(function, name)
        })
        .count()
}

fn rewrite_statement_list(statements: &mut Vec<Statement>, name: &str) -> Option<i64> {
    for index in 0..statements.len() {
        let Statement::Loop { body, .. } = &mut statements[index] else {
            continue;
        };
        let constants = loop_store_constants(body);
        for constant in constants {
            if rewrite_guarded_second_store(body, constant, name, false) {
                statements.insert(
                    index,
                    Statement::Assign {
                        name: name.to_owned(),
                        value: Expression::IntegerLiteral(constant),
                    },
                );
                return Some(constant);
            }
        }
    }

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
            rewrite_matching_store_constants(&mut statements[first..], constant, name);
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
            rewrite_matching_store_constants(
                &mut statements[first..=second],
                constant,
                name,
            );
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

/// Once a constant owns a call-crossing value version, reuse it for every
/// dominated store in that interval. This includes narrow member stores:
/// materializing a second literal into r0 would discard the retained value and
/// diverge from MWCC's shared-zero transaction.
fn rewrite_matching_store_constants(statements: &mut [Statement], constant: i64, name: &str) {
    for statement in statements {
        if store_integer_constant(statement) == Some(constant) {
            let Statement::Store { value, .. } = statement else {
                unreachable!("the matching constant was classified as a store")
            };
            *value = Expression::Variable(name.to_owned());
            continue;
        }
        match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                rewrite_matching_store_constants(then_body, constant, name);
                rewrite_matching_store_constants(else_body, constant, name);
            }
            Statement::Loop { body, .. } => {
                rewrite_matching_store_constants(body, constant, name);
            }
            _ => {}
        }
    }
}

fn loop_store_constants(statements: &[Statement]) -> Vec<i64> {
    let mut constants = Vec::new();
    for statement in statements {
        if let Some(constant) = store_integer_constant(statement) {
            if i32::try_from(constant).is_ok() && !constants.contains(&constant) {
                constants.push(constant);
            }
        }
        match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                for constant in loop_store_constants(then_body)
                    .into_iter()
                    .chain(loop_store_constants(else_body))
                {
                    if !constants.contains(&constant) {
                        constants.push(constant);
                    }
                }
            }
            Statement::Loop { body, .. } => {
                for constant in loop_store_constants(body) {
                    if !constants.contains(&constant) {
                        constants.push(constant);
                    }
                }
            }
            _ => {}
        }
    }
    constants
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
    use mwcc_syntax_trees::Parameter;

    fn pressure_function(parameter_count: usize) -> Function {
        let parameters: Vec<_> = (0..parameter_count)
            .map(|index| Parameter {
                parameter_type: Type::Int,
                name: format!("value{index}"),
            })
            .collect();
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
        statements.extend(
            parameters
                .iter()
                .map(|parameter| {
                    Statement::Expression(Expression::Variable(parameter.name.clone()))
                }),
        );
        Function {
            return_type: Type::Void,
            name: "pressure".into(),
            is_static: false,
            is_weak: false,
            parameters,
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

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
    fn versions_intervening_narrow_stores_with_the_retained_constant() {
        let mut statements = vec![
            Statement::Store {
                target: Expression::Variable("first".into()),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Store {
                target: Expression::Variable("narrow".into()),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Expression(Expression::Call {
                name: "flush".into(),
                arguments: Vec::new(),
            }),
            Statement::Store {
                target: Expression::Variable("last".into()),
                value: Expression::IntegerLiteral(0),
            },
        ];

        assert_eq!(
            rewrite_statement_list(&mut statements, "__retained"),
            Some(0)
        );
        assert!(statements[1..]
            .iter()
            .filter_map(|statement| match statement {
                Statement::Store { value, .. } => Some(value),
                _ => None,
            })
            .all(|value| matches!(value, Expression::Variable(name) if name == "__retained")));
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

    #[test]
    fn preserves_the_constant_lane_without_allocating_a_fifth_saved_home() {
        let pressured = pressure_function(MAX_RETAINED_CONSTANT_HOME_PRESSURE);

        assert!(repeated_store_constant_exceeds_home_capacity(&pressured, false));
        assert!(retain_repeated_store_constant_across_call(&pressured, false).is_none());
        assert!(!repeated_store_constant_exceeds_home_capacity(&pressured, true));
        assert!(retain_repeated_store_constant_across_call(&pressured, true).is_some());

        let available = pressure_function(MAX_RETAINED_CONSTANT_HOME_PRESSURE - 1);
        assert!(!repeated_store_constant_exceeds_home_capacity(&available, false));
        assert!(retain_repeated_store_constant_across_call(&available, false).is_some());
    }

    #[test]
    fn retains_a_store_constant_reused_by_a_call_bearing_loop() {
        let mut statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Variable("running".into())),
            step: None,
            body: vec![
                Statement::Expression(Expression::Call {
                    name: "destroy".into(),
                    arguments: Vec::new(),
                }),
                Statement::Store {
                    target: Expression::Variable("slot".into()),
                    value: Expression::IntegerLiteral(0),
                },
            ],
        }];

        assert_eq!(
            rewrite_statement_list(&mut statements, "__retained"),
            Some(0)
        );
        assert!(matches!(
            statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Loop { body, .. },
            ] if name == "__retained"
                && matches!(
                    body.as_slice(),
                    [
                        Statement::Expression(Expression::Call { .. }),
                        Statement::Store {
                            value: Expression::Variable(value),
                            ..
                        },
                    ] if value == name
                )
        ));
    }
}
