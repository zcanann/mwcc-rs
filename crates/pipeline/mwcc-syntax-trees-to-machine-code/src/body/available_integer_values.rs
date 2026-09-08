//! Reuse named word computations within store-free structured regions.
//!
//! Facts follow lexical dominance into an if arm. Stores, calls and joins
//! discard them; this intentionally needs no pointer-alias or global CFG model.

use crate::analysis::{expression_has_side_effect, expression_reads_name, structurally_equal};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Available {
    name: String,
    ty: Type,
    expression: Expression,
}

pub(super) fn reuse(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<Function> {
    if !function.guards.is_empty() || function.locals.is_empty() {
        return None;
    }
    let mut types = globals.clone();
    types.extend(
        function
            .parameters
            .iter()
            .map(|p| (p.name.clone(), p.parameter_type)),
    );
    types.extend(
        function
            .locals
            .iter()
            .map(|l| (l.name.clone(), l.declared_type)),
    );
    let mut volatile = volatile_globals.clone();
    volatile.extend(
        function
            .locals
            .iter()
            .filter(|l| l.is_volatile)
            .map(|l| l.name.clone()),
    );
    let address_taken = crate::frame::collect_address_taken(function);
    let carriers: HashSet<_> = function
        .locals
        .iter()
        .filter(|l| {
            !l.is_static
                && !l.is_volatile
                && !address_taken.contains(l.name.as_str())
                && matches!(l.declared_type, Type::Int | Type::UnsignedInt)
        })
        .map(|l| l.name.clone())
        .collect();
    if carriers.is_empty() {
        return None;
    }
    let mut rewritten = function.clone();
    let mut available = Vec::new();
    // Declaration initializers are entry definitions in the semantic tree.
    for local in &function.locals {
        if let Some(value) = &local.initializer {
            if expression_has_side_effect(value) {
                available.clear();
            }
            remember(
                &local.name,
                value,
                &mut available,
                &carriers,
                &types,
                &volatile,
            );
        }
    }
    let mut changed = false;
    rewrite_sequence(
        &mut rewritten.statements,
        &mut available,
        &carriers,
        &types,
        &volatile,
        &mut changed,
    );
    changed.then_some(rewritten)
}

fn word_expression(
    expression: &Expression,
    types: &HashMap<String, Type>,
    volatile: &HashSet<String>,
) -> bool {
    match expression {
        Expression::IntegerLiteral(_) => true,
        Expression::Variable(name) => {
            !volatile.contains(name)
                && matches!(types.get(name), Some(Type::Int | Type::UnsignedInt))
        }
        Expression::Binary { left, right, .. } => {
            word_expression(left, types, volatile) && word_expression(right, types, volatile)
        }
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => word_expression(operand, types, volatile),
        _ => false,
    }
}

fn remember(
    name: &str,
    value: &Expression,
    available: &mut Vec<Available>,
    carriers: &HashSet<String>,
    types: &HashMap<String, Type>,
    volatile: &HashSet<String>,
) {
    available.retain(|entry| entry.name != name && !expression_reads_name(&entry.expression, name));
    if carriers.contains(name)
        && matches!(value, Expression::Binary { .. })
        && word_expression(value, types, volatile)
        && !expression_reads_name(value, name)
        && !expression_has_side_effect(value)
    {
        available.push(Available {
            name: name.into(),
            ty: types[name],
            expression: value.clone(),
        });
    }
}

fn rewrite_sequence(
    statements: &mut [Statement],
    available: &mut Vec<Available>,
    carriers: &HashSet<String>,
    types: &HashMap<String, Type>,
    volatile: &HashSet<String>,
    changed: &mut bool,
) {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } => {
                if expression_has_side_effect(value) {
                    available.clear();
                }
                let original = value.clone();
                if carriers.contains(name) {
                    if let Some(entry) = available.iter().rev().find(|entry| {
                        Some(&entry.ty) == types.get(name)
                            && structurally_equal(&entry.expression, value)
                    }) {
                        *value = Expression::Variable(entry.name.clone());
                        *changed = true;
                    }
                }
                remember(name, &original, available, carriers, types, volatile);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                if expression_has_side_effect(condition) {
                    available.clear();
                }
                rewrite_sequence(
                    then_body,
                    &mut available.clone(),
                    carriers,
                    types,
                    volatile,
                    changed,
                );
                rewrite_sequence(
                    else_body,
                    &mut available.clone(),
                    carriers,
                    types,
                    volatile,
                    changed,
                );
                available.clear();
            }
            Statement::Expression(expression) if !expression_has_side_effect(expression) => {}
            // A label can be reached from another predecessor; a store can
            // alias any memory read; a call can change globals or escaped locals.
            _ => available.clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::BinaryOperator;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }
    fn quotient() -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Divide,
            left: Box::new(variable("input")),
            right: Box::new(Expression::IntegerLiteral(160)),
        }
    }
    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.into(),
            value,
        }
    }
    fn rewrite(mut statements: Vec<Statement>) -> Vec<Statement> {
        let carriers = ["first".into(), "second".into()].into_iter().collect();
        let types = [
            ("first".into(), Type::Int),
            ("second".into(), Type::Int),
            ("input".into(), Type::Int),
        ]
        .into_iter()
        .collect();
        rewrite_sequence(
            &mut statements,
            &mut Vec::new(),
            &carriers,
            &types,
            &HashSet::new(),
            &mut false,
        );
        statements
    }

    #[test]
    fn inherits_dominating_values_but_drops_facts_at_the_join() {
        let rewritten = rewrite(vec![
            assign("first", quotient()),
            Statement::If {
                condition: variable("first"),
                then_body: vec![assign("second", quotient())],
                else_body: vec![],
            },
            assign("second", quotient()),
        ]);
        let Statement::If { then_body, .. } = &rewritten[1] else {
            panic!()
        };
        assert!(
            matches!(&then_body[0], Statement::Assign { value: Expression::Variable(name), .. } if name == "first")
        );
        assert!(matches!(
            &rewritten[2],
            Statement::Assign {
                value: Expression::Binary { .. },
                ..
            }
        ));
    }

    #[test]
    fn stores_and_carrier_redefinitions_invalidate_a_snapshot() {
        for barrier in [
            Statement::Store {
                target: variable("input"),
                value: Expression::IntegerLiteral(320),
            },
            assign("first", Expression::IntegerLiteral(7)),
        ] {
            let rewritten = rewrite(vec![
                assign("first", quotient()),
                barrier,
                assign("second", quotient()),
            ]);
            assert!(matches!(
                &rewritten[2],
                Statement::Assign {
                    value: Expression::Binary { .. },
                    ..
                }
            ));
        }
    }
}
