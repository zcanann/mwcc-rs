//! Straight-line value versions for reassigned structured-frame locals.
//!
//! MWCC colors definitions, not source identifiers. A local assigned twice can
//! therefore occupy two unrelated homes, while each expired definition can be
//! reused by a later value. The general structured emitter tracks locations by
//! name, so this conservative normalization gives each proven straight-line
//! redefinition a private internal name before liveness and home coloring.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

pub(super) fn split_reassigned_local_versions(function: &Function) -> Option<Function> {
    if function
        .statements
        .iter()
        .any(contains_unsupported_version_control_flow)
    {
        return None;
    }

    let address_taken = crate::frame::collect_address_taken(function);
    let call_accumulators = super::structured_call_accumulator::call_accumulator_names(function);
    let mut definitions = std::collections::HashMap::<String, usize>::new();
    for local in &function.locals {
        if local.initializer.is_some() {
            definitions.insert(local.name.clone(), 1);
        }
    }
    for statement in &function.statements {
        if let Statement::Assign { name, value } = statement {
            if !expression_reads_name(value, name) {
                *definitions.entry(name.clone()).or_default() += 1;
            }
        }
    }
    let candidates: std::collections::HashSet<String> = function
        .locals
        .iter()
        .filter(|local| {
            local.array_length.is_none()
                && !local.is_static
                && !local.is_volatile
                && !address_taken.contains(local.name.as_str())
                && !call_accumulators.contains(local.name.as_str())
                && function.statements.iter().any(|statement| {
                    matches!(statement,
                        Statement::Assign { name, value: Expression::Call { .. } }
                            if name == &local.name)
                })
                && definitions.get(&local.name).copied().unwrap_or_default() >= 2
        })
        .map(|local| local.name.clone())
        .collect();
    if candidates.is_empty() {
        return None;
    }

    let declarations: std::collections::HashMap<_, _> = function
        .locals
        .iter()
        .map(|local| (local.name.as_str(), local))
        .collect();
    let mut occupied: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut active: std::collections::HashMap<String, Expression> = candidates
        .iter()
        .map(|name| (name.clone(), Expression::Variable(name.clone())))
        .collect();
    let mut seen_definitions: std::collections::HashMap<String, usize> = function
        .locals
        .iter()
        .filter(|local| local.initializer.is_some() && candidates.contains(&local.name))
        .map(|local| (local.name.clone(), 1))
        .collect();
    let mut rewritten = function.clone();
    let mut added_locals = Vec::new();
    rewritten.statements = function
        .statements
        .iter()
        .map(|statement| {
            let Statement::Assign { name, value } = statement else {
                return substitute_statement_reads(statement, &active);
            };
            let self_update = expression_reads_name(value, name);
            let value = crate::value_tracking::substitute(value, &active);
            if !candidates.contains(name) {
                return Statement::Assign {
                    name: name.clone(),
                    value,
                };
            }
            if self_update {
                let target = active
                    .get(name)
                    .and_then(|expression| match expression {
                        Expression::Variable(name) => Some(name.clone()),
                        _ => None,
                    })
                    .expect("active value versions are variables");
                return Statement::Assign {
                    name: target,
                    value,
                };
            }

            let seen = seen_definitions.entry(name.clone()).or_default();
            if *seen == 0 {
                *seen = 1;
                return Statement::Assign {
                    name: name.clone(),
                    value,
                };
            }
            let ordinal = *seen;
            *seen += 1;
            let version = unique_version_name(name, ordinal, &mut occupied);
            let mut declaration = (*declarations[name.as_str()]).clone();
            declaration.name = version.clone();
            declaration.initializer = None;
            added_locals.push(declaration);
            active.insert(name.clone(), Expression::Variable(version.clone()));
            Statement::Assign {
                name: version,
                value,
            }
        })
        .collect();
    rewritten.locals.extend(added_locals);
    if let Some(expression) = &function.return_expression {
        rewritten.return_expression = Some(crate::value_tracking::substitute(expression, &active));
    }
    fold_single_use_constant_versions(&mut rewritten);
    Some(rewritten)
}

/// Find a different source value read by a reassignment and still needed
/// afterward. The emitter combines this proof with its location table: when
/// source and destination share a home, the write must break that alias.
pub(super) fn reassignment_live_source<'a>(
    function: &'a Function,
    assigned: &str,
    value: &Expression,
    remaining_statements: &[Statement],
) -> Option<&'a str> {
    function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .chain(
            function
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str()),
        )
        .find(|candidate| {
            *candidate != assigned
                && expression_reads_name(value, candidate)
                && body_uses_local(remaining_statements, candidate)
        })
}

fn fold_single_use_constant_versions(function: &mut Function) {
    let versions: Vec<_> = function
        .locals
        .iter()
        .filter(|local| local.name.starts_with("__mwcc_value_"))
        .map(|local| local.name.clone())
        .collect();
    for version in versions {
        let Some((assignment_index, value)) =
            function
                .statements
                .iter()
                .enumerate()
                .find_map(|(index, statement)| match statement {
                    Statement::Assign { name, value }
                        if name == &version && matches!(value, Expression::IntegerLiteral(_)) =>
                    {
                        Some((index, value.clone()))
                    }
                    _ => None,
                })
        else {
            continue;
        };
        let reads: usize = function
            .statements
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != assignment_index)
            .map(|(_, statement)| statement_name_occurrences(statement, &version))
            .sum::<usize>()
            + function
                .return_expression
                .as_ref()
                .map_or(0, |expression| count_name_occurrences(expression, &version));
        if reads != 1 {
            continue;
        }
        function.statements.remove(assignment_index);
        let values = std::collections::HashMap::from([(version.clone(), value)]);
        function.statements = function
            .statements
            .iter()
            .map(|statement| substitute_statement_reads(statement, &values))
            .collect();
        if let Some(expression) = &function.return_expression {
            function.return_expression =
                Some(crate::value_tracking::substitute(expression, &values));
        }
    }
}

fn statement_name_occurrences(statement: &Statement, name: &str) -> usize {
    match statement {
        Statement::Store { target, value } => {
            count_name_occurrences(target, name) + count_name_occurrences(value, name)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            count_name_occurrences(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            count_name_occurrences(condition, name)
                + then_body
                    .iter()
                    .map(|statement| statement_name_occurrences(statement, name))
                    .sum::<usize>()
                + else_body
                    .iter()
                    .map(|statement| statement_name_occurrences(statement, name))
                    .sum::<usize>()
        }
        Statement::Return(Some(value)) => count_name_occurrences(value, name),
        _ => 0,
    }
}

fn unique_version_name(
    name: &str,
    ordinal: usize,
    occupied: &mut std::collections::HashSet<String>,
) -> String {
    let mut suffix = ordinal;
    loop {
        let candidate = format!("__mwcc_value_{name}_{suffix}");
        if occupied.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn contains_unsupported_version_control_flow(statement: &Statement) -> bool {
    matches!(statement, Statement::Loop { .. } | Statement::Switch { .. })
        || match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => then_body.iter().chain(else_body).any(|statement| {
                matches!(statement, Statement::Assign { .. })
                    || contains_unsupported_version_control_flow(statement)
            }),
            _ => false,
        }
}

fn substitute_statement_reads(
    statement: &Statement,
    values: &std::collections::HashMap<String, Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: crate::value_tracking::substitute(target, values),
            value: crate::value_tracking::substitute(value, values),
        },
        Statement::Expression(expression) => {
            Statement::Expression(crate::value_tracking::substitute(expression, values))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: crate::value_tracking::substitute(condition, values),
            then_body: then_body
                .iter()
                .map(|statement| substitute_statement_reads(statement, values))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| substitute_statement_reads(statement, values))
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|expression| crate::value_tracking::substitute(expression, values)),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![local("value", None)],
            statements: vec![
                Statement::Assign {
                    name: "value".into(),
                    value: Expression::IntegerLiteral(2),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("value".into())],
                }),
                Statement::Assign {
                    name: "value".into(),
                    value: Expression::Call {
                        name: "produce".into(),
                        arguments: Vec::new(),
                    },
                },
                Statement::Expression(Expression::Call {
                    name: "side_effect".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("value".into())],
                }),
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("value".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn gives_a_redefinition_a_private_name_and_rewrites_later_reads() {
        let rewritten = split_reassigned_local_versions(&function()).unwrap();
        let version = &rewritten.locals[1].name;
        assert_ne!(version, "value");
        assert!(matches!(
            &rewritten.statements[2],
            Statement::Assign { name, .. } if name == version
        ));
        assert!(matches!(
            &rewritten.statements[4],
            Statement::Expression(Expression::Call { arguments, .. })
                if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == version)
        ));
        assert!(matches!(
            rewritten.return_expression,
            Some(Expression::Variable(name)) if name == *version
        ));
    }

    #[test]
    fn finds_a_reassignment_source_that_remains_live() {
        let mut function = function();
        function.locals = vec![local("dummy", None), local("length", None)];
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable("dummy".into())),
            right: Box::new(Expression::IntegerLiteral(20)),
        };
        let remaining = [Statement::Expression(Expression::Variable("dummy".into()))];

        assert_eq!(
            reassignment_live_source(&function, "length", &value, &remaining),
            Some("dummy"),
        );
    }
}
