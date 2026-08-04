//! Repeated floating selections that are invariant across a structured loop.
//!
//! Mixer-style loops commonly select both `x` and `1.0f - x` from two switch
//! packets before calling a scalar transform. Optimized MWCC builds the three
//! complements once, retains the bounds once, and lets every switch arm select
//! a saved value. Exposing those source-proven invariants as generated locals
//! keeps expression selection separate from register allocation and works for
//! any equivalent pair of switch packets.

#[allow(unused_imports)]
use super::*;

const PREFIX: &str = "__mwcc_loop_float_invariant_";

/// MWCC keeps the switch-selected call argument distinct from the loop's
/// post-call accumulator even though their source lifetimes do not overlap.
/// The uncoalesced lane remains part of the ABI-contiguous saved-FPR range.
pub(super) fn retains_separate_selection_lane(function: &Function) -> bool {
    function
        .locals
        .iter()
        .any(|local| local.name.starts_with(PREFIX))
}

pub(super) fn hoist_repeated_float_switch_invariants(function: &Function) -> Option<Function> {
    let types: std::collections::HashMap<&str, Type> = function
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.parameter_type))
        .chain(
            function
                .locals
                .iter()
                .map(|local| (local.name.as_str(), local.declared_type)),
        )
        .collect();
    let volatile: std::collections::HashSet<&str> = function
        .locals
        .iter()
        .filter(|local| local.is_volatile)
        .map(|local| local.name.as_str())
        .collect();
    let address_taken = crate::frame::collect_address_taken(function);
    let mut available: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(
            function
                .locals
                .iter()
                .filter(|local| local.initializer.is_some())
                .map(|local| local.name.clone()),
        )
        .collect();
    let mut used: std::collections::HashSet<String> = types
        .keys()
        .map(|name| (*name).to_owned())
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let (statements, changed) = rewrite_sequence(
        &function.statements,
        &types,
        &volatile,
        &address_taken,
        &mut available,
        &mut used,
        &mut declarations,
        &mut next_name,
    );
    changed.then(|| {
        let mut rewritten = function.clone();
        rewritten.locals.extend(declarations);
        rewritten.statements = statements;
        rewritten
    })
}

#[allow(clippy::too_many_arguments)]
fn rewrite_sequence(
    statements: &[Statement],
    types: &std::collections::HashMap<&str, Type>,
    volatile: &std::collections::HashSet<&str>,
    address_taken: &std::collections::HashSet<String>,
    available: &mut std::collections::HashSet<String>,
    used: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(statements.len());
    let mut changed = false;
    for statement in statements {
        if let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        {
            if let Some(plan) = plan_loop(
                body,
                types,
                volatile,
                address_taken,
                available,
                used,
                declarations,
                next_name,
            ) {
                output.extend(plan.prefix);
                output.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body: plan.body,
                });
                changed = true;
                continue;
            }
        }
        output.push(statement.clone());
        collect_assigned_names(statement, available);
    }
    (output, changed)
}

struct LoopRewrite {
    prefix: Vec<Statement>,
    body: Vec<Statement>,
}

#[allow(clippy::too_many_arguments)]
fn plan_loop(
    body: &[Statement],
    types: &std::collections::HashMap<&str, Type>,
    volatile: &std::collections::HashSet<&str>,
    address_taken: &std::collections::HashSet<String>,
    available: &std::collections::HashSet<String>,
    used: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> Option<LoopRewrite> {
    if switch_count(body) < 2 || !contains_call(body) {
        return None;
    }

    let mut sources: Vec<(String, usize)> = Vec::new();
    let mut zero_count = 0usize;
    let mut one_count = 0usize;
    for statement in body {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            match expression {
                Expression::FloatLiteral(value) if *value == 0.0 => zero_count += 1,
                Expression::FloatLiteral(value) if *value == 1.0 => one_count += 1,
                _ => {}
            }
            let Some(source) = one_minus_float_variable(expression) else {
                return;
            };
            if let Some((_, count)) = sources.iter_mut().find(|(name, _)| name == source) {
                *count += 1;
            } else {
                sources.push((source.to_owned(), 1));
            }
        });
    }
    sources.retain(|(source, count)| {
        *count >= 2
            && types.get(source.as_str()) == Some(&Type::Float)
            && available.contains(source)
            && !volatile.contains(source.as_str())
            && !address_taken.contains(source)
            && !super::structured_expression_visit::statements_assign_name(body, source)
    });
    if sources.len() < 3 || zero_count < 2 || one_count < 2 {
        return None;
    }

    let one = fresh_name(used, next_name);
    let zero = fresh_name(used, next_name);
    declarations.push(float_local(&one));
    declarations.push(float_local(&zero));
    let mut prefix = vec![
        Statement::Assign {
            name: one.clone(),
            value: Expression::FloatLiteral(1.0),
        },
        Statement::Assign {
            name: zero.clone(),
            value: Expression::FloatLiteral(0.0),
        },
    ];
    let mut complements = Vec::with_capacity(sources.len());
    for (source, _) in sources {
        let name = fresh_name(used, next_name);
        declarations.push(float_local(&name));
        prefix.push(Statement::Assign {
            name: name.clone(),
            value: Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(Expression::Variable(one.clone())),
                right: Box::new(Expression::Variable(source.clone())),
            },
        });
        complements.push((source, name));
    }
    let body = body
        .iter()
        .map(|statement| {
            super::structured_expression_visit::rewrite_statement(statement, &mut |expression| {
                if let Some(source) = one_minus_float_variable(expression) {
                    if let Some((_, replacement)) =
                        complements.iter().find(|(name, _)| name == source)
                    {
                        return Some(Expression::Variable(replacement.clone()));
                    }
                }
                match expression {
                    Expression::FloatLiteral(value) if *value == 0.0 => {
                        Some(Expression::Variable(zero.clone()))
                    }
                    Expression::FloatLiteral(value) if *value == 1.0 => {
                        Some(Expression::Variable(one.clone()))
                    }
                    _ => None,
                }
            })
        })
        .collect();
    Some(LoopRewrite { prefix, body })
}

fn one_minus_float_variable(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    else {
        return None;
    };
    matches!(left.as_ref(), Expression::FloatLiteral(value) if *value == 1.0)
        .then(|| match right.as_ref() {
            Expression::Variable(name) => Some(name.as_str()),
            _ => None,
        })
        .flatten()
}

fn contains_call(statements: &[Statement]) -> bool {
    let mut found = false;
    for statement in statements {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            found |= matches!(
                expression,
                Expression::Call { .. }
                    | Expression::CallThrough { .. }
                    | Expression::VirtualCall { .. }
            );
        });
    }
    found
}

fn switch_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Switch { arms, default, .. } => {
                1 + arms
                    .iter()
                    .map(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => switch_count(body),
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
                    .sum::<usize>()
                    + default.as_ref().map_or(0, |body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => switch_count(body),
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => switch_count(then_body) + switch_count(else_body),
            Statement::Loop { body, .. } => switch_count(body),
            _ => 0,
        })
        .sum()
}

fn collect_assigned_names(
    statement: &Statement,
    assigned: &mut std::collections::HashSet<String>,
) {
    match statement {
        Statement::Assign { name, .. } => {
            assigned.insert(name.clone());
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            for statement in then_body.iter().chain(else_body) {
                collect_assigned_names(statement, assigned);
            }
        }
        Statement::Switch { arms, default, .. } => {
            for body in arms.iter().map(|arm| &arm.body).chain(default) {
                if let mwcc_syntax_trees::ArmBody::Statements(statements) = body {
                    for statement in statements {
                        collect_assigned_names(statement, assigned);
                    }
                }
            }
        }
        Statement::Loop { body, .. } => {
            for statement in body {
                collect_assigned_names(statement, assigned);
            }
        }
        _ => {}
    }
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next_name: &mut usize) -> String {
    loop {
        let name = format!("{PREFIX}{}", *next_name);
        *next_name += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

fn float_local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::Float,
        name: name.to_owned(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    }
}

#[cfg(test)]
#[path = "structured_loop_float_invariants_tests.rs"]
mod tests;
