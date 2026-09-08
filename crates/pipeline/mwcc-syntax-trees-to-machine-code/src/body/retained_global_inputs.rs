//! Retain repeated word-global operands along store-free structured paths.
//!
//! A capture is placed at an existing unconditional read, never at a speculative
//! branch entry. Both arms must preserve memory for a fact to survive their join.

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::{HashMap, HashSet};

pub(super) fn reuse(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<Function> {
    if !function.guards.is_empty() || !function.inline_asm_blocks.is_empty() {
        return None;
    }
    let escaped = crate::frame::collect_address_taken(function);
    let locals: HashMap<_, _> = function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static
                && local.array_length.is_none()
                && !local.is_volatile
                && !escaped.contains(local.name.as_str())
                && matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        })
        .map(|local| (local.name.clone(), local.clone()))
        .collect();
    if locals.is_empty() {
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
    let mut candidates = globals.clone();
    candidates.retain(|name, ty| {
        !volatile_globals.contains(name) && matches!(ty, Type::Int | Type::UnsignedInt)
    });
    for name in function
        .locals
        .iter()
        .map(|l| &l.name)
        .chain(function.parameters.iter().map(|p| &p.name))
    {
        candidates.remove(name);
    }
    let mut volatile = volatile_globals.clone();
    volatile.extend(
        function
            .locals
            .iter()
            .filter(|l| l.is_volatile)
            .map(|l| l.name.clone()),
    );
    let mut rewritten = function.clone();
    let mut names: HashSet<_> = types.keys().cloned().collect();
    let mut added = Vec::new();
    capture_sequence(
        &mut rewritten.statements,
        &locals,
        &types,
        &candidates,
        &volatile,
        &mut names,
        &mut added,
    );
    if added.is_empty() {
        return None;
    }
    rewritten.locals.extend(added);
    Some(rewritten)
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
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            // Short-circuit operands are not unconditional reads.
            !matches!(
                operator,
                mwcc_syntax_trees::BinaryOperator::LogicalAnd
                    | mwcc_syntax_trees::BinaryOperator::LogicalOr
            ) && word_expression(left, types, volatile)
                && word_expression(right, types, volatile)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => word_expression(operand, types, volatile),
        _ => false,
    }
}

fn collect_globals(
    expression: &Expression,
    globals: &HashMap<String, Type>,
    names: &mut Vec<String>,
) {
    match expression {
        Expression::Variable(name) if globals.contains_key(name) => {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        Expression::Binary { left, right, .. } => {
            collect_globals(left, globals, names);
            collect_globals(right, globals, names);
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            collect_globals(operand, globals, names)
        }
        _ => {}
    }
}

fn replace_reads(expression: &mut Expression, global: &str, captured: &str) -> usize {
    match expression {
        Expression::Variable(name) if name == global => {
            *name = captured.into();
            1
        }
        Expression::Binary { left, right, .. } => {
            replace_reads(left, global, captured) + replace_reads(right, global, captured)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            replace_reads(operand, global, captured)
        }
        _ => 0,
    }
}

// Only these expression forms are known to contain no calls or writes. A
// conservative whitelist also keeps embedded updates in unfamiliar nodes out.
fn read_only(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_) | Expression::IntegerLiteral(_) => true,
        Expression::Binary { left, right, .. } => read_only(left) && read_only(right),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand } => read_only(operand),
        Expression::Index { base, index } => read_only(base) && read_only(index),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => read_only(base),
        _ => false,
    }
}

fn has_label(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Label(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => has_label(then_body) || has_label(else_body),
        // Unmodeled control flow may contain an incoming label.
        Statement::Loop { .. } | Statement::Switch { .. } => true,
        _ => false,
    })
}

fn reuse_sequence(
    statements: &mut [Statement],
    global: &str,
    captured: &str,
    locals: &HashMap<String, LocalDeclaration>,
    count: &mut usize,
) -> bool {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } if read_only(value) => {
                *count += replace_reads(value, global, captured);
                if !locals.contains_key(name) {
                    return false;
                }
            }
            Statement::Store { target, value } if read_only(target) && read_only(value) => {
                *count += replace_reads(value, global, captured);
                return false;
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } if read_only(condition) && !has_label(then_body) && !has_label(else_body) => {
                *count += replace_reads(condition, global, captured);
                let then_live = reuse_sequence(then_body, global, captured, locals, count);
                let else_live = reuse_sequence(else_body, global, captured, locals, count);
                if !then_live || !else_live {
                    return false;
                }
            }
            Statement::Expression(value) if read_only(value) => {
                *count += replace_reads(value, global, captured);
            }
            Statement::Return(Some(value)) if read_only(value) => {
                *count += replace_reads(value, global, captured);
                return false;
            }
            _ => return false,
        }
    }
    true
}

fn capture_sequence(
    statements: &mut Vec<Statement>,
    locals: &HashMap<String, LocalDeclaration>,
    types: &HashMap<String, Type>,
    globals: &HashMap<String, Type>,
    volatile: &HashSet<String>,
    names: &mut HashSet<String>,
    added: &mut Vec<LocalDeclaration>,
) {
    let mut i = 0;
    while i < statements.len() {
        let seed = match &statements[i] {
            Statement::Assign {
                name,
                value: value @ Expression::Binary { .. },
            } if locals.contains_key(name) && word_expression(value, types, volatile) => {
                let mut inputs = Vec::new();
                collect_globals(value, globals, &mut inputs);
                Some((locals[name].clone(), inputs))
            }
            _ => None,
        };
        if let Some((template, inputs)) = seed {
            for global in inputs {
                let mut suffix = statements[i + 1..].to_vec();
                let mut number = added.len();
                let captured = loop {
                    let name = format!("__mwcc_retained_input_{number}");
                    if !names.contains(&name) {
                        break name;
                    }
                    number += 1;
                };
                let mut count = 0;
                reuse_sequence(&mut suffix, &global, &captured, locals, &mut count);
                if count == 0 {
                    continue;
                }
                statements[i + 1..].clone_from_slice(&suffix);
                if let Statement::Assign { value, .. } = &mut statements[i] {
                    replace_reads(value, &global, &captured);
                }
                statements.insert(
                    i,
                    Statement::Assign {
                        name: captured.clone(),
                        value: Expression::Variable(global.clone()),
                    },
                );
                let mut local = template.clone();
                local.name = captured.clone();
                local.declared_type = globals[&global];
                local.initializer = None;
                names.insert(captured);
                added.push(local);
                i += 1;
            }
        }
        if let Statement::If {
            then_body,
            else_body,
            ..
        } = &mut statements[i]
        {
            capture_sequence(then_body, locals, types, globals, volatile, names, added);
            capture_sequence(else_body, locals, types, globals, volatile, names, added);
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::BinaryOperator;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }
    fn local() -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: "q".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: vec![],
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }
    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.into(),
            value,
        }
    }
    fn store() -> Statement {
        Statement::Store {
            target: Expression::Dereference {
                pointer: Box::new(variable("out")),
            },
            value: variable("input"),
        }
    }
    fn branch(body: Vec<Statement>) -> Statement {
        Statement::If {
            condition: variable("flag"),
            then_body: body,
            else_body: vec![],
        }
    }

    #[test]
    fn captures_at_the_original_read_and_survives_local_only_joins() {
        let mut statements = vec![
            assign(
                "q",
                Expression::Binary {
                    operator: BinaryOperator::Divide,
                    left: Box::new(variable("input")),
                    right: Box::new(Expression::IntegerLiteral(160)),
                },
            ),
            branch(vec![assign("q", Expression::IntegerLiteral(7))]),
            store(),
        ];
        let locals = [("q".into(), local())].into_iter().collect();
        let types = [("q".into(), Type::Int), ("input".into(), Type::Int)]
            .into_iter()
            .collect();
        let globals = [("input".into(), Type::Int)].into_iter().collect();
        let mut names = ["__mwcc_retained_input_0".into()].into_iter().collect();
        let mut added = Vec::new();
        capture_sequence(
            &mut statements,
            &locals,
            &types,
            &globals,
            &HashSet::new(),
            &mut names,
            &mut added,
        );
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "__mwcc_retained_input_1");
        assert!(
            matches!(&statements[0], Statement::Assign { value: Expression::Variable(name), .. } if name == "input")
        );
        assert!(
            matches!(&statements[3], Statement::Store { value: Expression::Variable(name), .. } if name == &added[0].name)
        );
    }

    #[test]
    fn stores_calls_and_incoming_labels_block_join_reuse() {
        for barrier in [
            Statement::Store {
                target: variable("out"),
                value: Expression::IntegerLiteral(19),
            },
            assign("input", Expression::IntegerLiteral(23)),
            Statement::Expression(Expression::Call {
                name: "mutate".into(),
                arguments: vec![],
            }),
            Statement::Label("incoming".into()),
        ] {
            let mut statements = vec![branch(vec![barrier]), store()];
            let mut count = 0;
            reuse_sequence(
                &mut statements,
                "input",
                "saved",
                &HashMap::new(),
                &mut count,
            );
            assert_eq!(count, 0);
            assert!(
                matches!(&statements[1], Statement::Store { value: Expression::Variable(name), .. } if name == "input")
            );
        }
    }

    #[test]
    fn store_consumes_the_snapshot_before_invalidating_it() {
        let mut statements = vec![store(), store()];
        let mut count = 0;
        assert!(!reuse_sequence(
            &mut statements,
            "input",
            "saved",
            &HashMap::new(),
            &mut count
        ));
        assert_eq!(count, 1);
        assert!(
            matches!(&statements[0], Statement::Store { value: Expression::Variable(name), .. } if name == "saved")
        );
        assert!(
            matches!(&statements[1], Statement::Store { value: Expression::Variable(name), .. } if name == "input")
        );
    }

    #[test]
    fn volatile_and_short_circuit_operands_cannot_seed_captures() {
        let types = [("input".into(), Type::Int)].into_iter().collect();
        let volatile = ["input".into()].into_iter().collect();
        assert!(!word_expression(&variable("input"), &types, &volatile));
        for operator in [BinaryOperator::LogicalAnd, BinaryOperator::LogicalOr] {
            let expression = Expression::Binary {
                operator,
                left: Box::new(Expression::IntegerLiteral(0)),
                right: Box::new(variable("input")),
            };
            assert!(!word_expression(&expression, &types, &HashSet::new()));
        }
    }
}
