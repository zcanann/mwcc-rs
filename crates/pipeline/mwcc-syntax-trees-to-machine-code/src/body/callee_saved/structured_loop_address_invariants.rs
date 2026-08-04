//! File-scope addresses retained across calls in a structured loop.
//!
//! A direct call argument such as `suspend(&thread)` is loop invariant. When an
//! earlier call in the loop makes the materialized address cross a call edge,
//! optimized MWCC hoists it before the loop and gives it an ordinary saved local
//! home. This normalization exposes that value lifetime to the shared planner;
//! instruction selection remains owned by the normal address and call emitters.

use super::*;

pub(super) fn hoist_loop_address_invariants(function: &Function) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let source_locals = used.clone();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let mut changed = false;
    let mut statements = Vec::with_capacity(function.statements.len());

    for statement in &function.statements {
        let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        else {
            statements.push(statement.clone());
            continue;
        };
        let candidates = retained_call_argument_addresses(body, &source_locals);
        if candidates.is_empty() {
            statements.push(statement.clone());
            continue;
        }

        let replacements = candidates
            .into_iter()
            .map(|symbol| {
                let name = fresh_name(&mut used, &mut next_name);
                declarations.push(LocalDeclaration {
                    declared_type: Type::Pointer(Pointee::UnsignedChar),
                    name: name.clone(),
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
                statements.push(Statement::Assign {
                    name: name.clone(),
                    value: Expression::AddressOf {
                        operand: Box::new(Expression::Variable(symbol.clone())),
                    },
                });
                (symbol, name)
            })
            .collect::<std::collections::HashMap<_, _>>();
        statements.push(Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body: body
                .iter()
                .map(|statement| rewrite_statement(statement, &replacements))
                .collect(),
        });
        changed = true;
    }

    changed.then(|| {
        let mut hoisted = function.clone();
        hoisted.locals.extend(declarations);
        hoisted.statements = statements;
        hoisted
    })
}

fn retained_call_argument_addresses(
    body: &[Statement],
    source_locals: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut call_ordinal = 0usize;
    let mut candidates = Vec::new();
    for statement in body {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            let Expression::Call { arguments, .. } = expression else {
                return;
            };
            if call_ordinal != 0 {
                for argument in arguments {
                    if let Expression::AddressOf { operand } = argument {
                        if let Expression::Variable(symbol) = operand.as_ref() {
                            if !source_locals.contains(symbol) && !candidates.contains(symbol) {
                                candidates.push(symbol.clone());
                            }
                        }
                    }
                }
            }
            call_ordinal += 1;
        });
    }
    candidates
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next: &mut usize) -> String {
    loop {
        let name = format!("__mwcc_loop_address_{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

fn rewrite_statement(
    statement: &Statement,
    replacements: &std::collections::HashMap<String, String>,
) -> Statement {
    super::structured_expression_visit::rewrite_statement(statement, &mut |expression| {
        let Expression::AddressOf { operand } = expression else {
            return None;
        };
        let Expression::Variable(symbol) = operand.as_ref() else {
            return None;
        };
        replacements
            .get(symbol)
            .map(|name| Expression::Variable(name.clone()))
    })
}
