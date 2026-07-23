//! Translation-unit symbol-reference analysis used by emission orchestration.
//!
//! Deferred static-inline candidates are speculative until a later source item
//! references them. This pass records those references before code generation,
//! so a dead candidate never has to lower merely to be discarded afterwards.

use mwcc_syntax_trees::{ArmBody, AsmItem, AsmOperand, Expression, Statement, TranslationUnit};
use std::collections::{HashMap, HashSet};

pub(crate) fn referenced_function_candidates(
    unit: &TranslationUnit,
    candidates: &HashSet<String>,
) -> HashSet<String> {
    referenced_candidates(unit, candidates)
}

/// Reachable inline definitions when `-inline off` turns call-site composition
/// into ordinary out-of-line calls. Unlike speculative materialization, skipped
/// inline bodies are graph nodes rather than roots: only an emitted function or
/// data relocation can pull one (and its transitive callees) into the object.
pub(crate) fn referenced_disabled_inlines(unit: &TranslationUnit) -> HashSet<String> {
    let mut candidates: HashSet<String> = unit
        .skipped_inline_definitions
        .iter()
        .map(|function| function.name.clone())
        .collect();
    candidates.extend(unit.materialized_inline_candidates.iter().cloned());
    referenced_candidates(unit, &candidates)
}

/// Find candidate definitions reachable from an ordinary emitted function or
/// data reference. Recovered skipped-inline definitions are graph nodes rather
/// than roots: a real caller may pull their transitive dependencies, but a dead
/// header-only inline chain cannot create object code by referring to itself.
fn referenced_candidates(
    unit: &TranslationUnit,
    candidates: &HashSet<String>,
) -> HashSet<String> {
    let mut graph_nodes = candidates.clone();
    graph_nodes.extend(
        unit.skipped_inline_definitions
            .iter()
            .map(|function| function.name.clone()),
    );
    let mut referenced = HashSet::new();
    let mut candidate_edges = HashMap::new();
    for function in &unit.functions {
        let outgoing = function_candidate_references(function, &graph_nodes);
        if candidates.contains(&function.name) {
            candidate_edges
                .entry(function.name.clone())
                .or_insert(outgoing);
        } else {
            referenced.extend(outgoing);
        }
    }
    for function in &unit.skipped_inline_definitions {
        candidate_edges.insert(
            function.name.clone(),
            function_candidate_references(function, &graph_nodes),
        );
    }
    for global in &unit.globals {
        for (_, target, _) in &global.data_relocations {
            if graph_nodes.contains(target) {
                referenced.insert(target.clone());
            }
        }
        if let Some(elements) = &global.address_initializer {
            for element in elements {
                if let mwcc_syntax_trees::PointerElement::Symbol(target) = element {
                    if graph_nodes.contains(target) {
                        referenced.insert(target.clone());
                    }
                }
            }
        }
    }
    extend_reachable_candidates(&mut referenced, &candidate_edges);
    referenced.retain(|name| candidates.contains(name));
    referenced
}

fn extend_reachable_candidates(
    referenced: &mut HashSet<String>,
    candidate_edges: &HashMap<String, HashSet<String>>,
) {
    let mut frontier: Vec<String> = referenced.iter().cloned().collect();
    while let Some(candidate) = frontier.pop() {
        let Some(outgoing) = candidate_edges.get(&candidate) else {
            continue;
        };
        for target in outgoing {
            if referenced.insert(target.clone()) {
                frontier.push(target.clone());
            }
        }
    }
}

fn function_candidate_references(
    function: &mwcc_syntax_trees::Function,
    candidates: &HashSet<String>,
) -> HashSet<String> {
    let mut referenced = HashSet::new();
    let owner = function.name.as_str();
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            collect_expression(initializer, owner, candidates, &mut referenced);
        }
    }
    for guard in &function.guards {
        collect_expression(&guard.condition, owner, candidates, &mut referenced);
        collect_expression(&guard.value, owner, candidates, &mut referenced);
    }
    if let Some(value) = &function.return_expression {
        collect_expression(value, owner, candidates, &mut referenced);
    }
    for statement in &function.statements {
        collect_statement(statement, owner, candidates, &mut referenced);
    }
    if let Some(items) = &function.asm_body {
        for item in items {
            let AsmItem::Instruction(instruction) = item else {
                continue;
            };
            for operand in &instruction.operands {
                let target = match operand {
                    AsmOperand::Label(name)
                    | AsmOperand::Symbol { name, .. }
                    | AsmOperand::SymbolMemory { name, .. } => Some(name),
                    _ => None,
                };
                if let Some(target) = target {
                    record(target, owner, candidates, &mut referenced);
                }
            }
        }
    }
    referenced
}

fn record(name: &str, owner: &str, candidates: &HashSet<String>, referenced: &mut HashSet<String>) {
    if name != owner && candidates.contains(name) {
        referenced.insert(name.to_owned());
    }
}

fn collect_statement(
    statement: &Statement,
    owner: &str,
    candidates: &HashSet<String>,
    referenced: &mut HashSet<String>,
) {
    match statement {
        Statement::Store { target, value } => {
            collect_expression(target, owner, candidates, referenced);
            collect_expression(value, owner, candidates, referenced);
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            collect_expression(value, owner, candidates, referenced);
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            collect_expression(condition, owner, candidates, referenced);
            for statement in then_body.iter().chain(else_body) {
                collect_statement(statement, owner, candidates, referenced);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                collect_expression(value, owner, candidates, referenced);
            }
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            collect_expression(scrutinee, owner, candidates, referenced);
            for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
                match body {
                    ArmBody::Return(value) => {
                        collect_expression(value, owner, candidates, referenced)
                    }
                    ArmBody::Statements(statements) => {
                        for statement in statements {
                            collect_statement(statement, owner, candidates, referenced);
                        }
                    }
                }
            }
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            for expression in [initializer, condition, step].into_iter().flatten() {
                collect_expression(expression, owner, candidates, referenced);
            }
            for statement in body {
                collect_statement(statement, owner, candidates, referenced);
            }
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {}
    }
}

fn collect_expression(
    expression: &Expression,
    owner: &str,
    candidates: &HashSet<String>,
    referenced: &mut HashSet<String>,
) {
    match expression {
        Expression::Variable(name) => record(name, owner, candidates, referenced),
        Expression::Call { name, arguments } => {
            record(name, owner, candidates, referenced);
            for argument in arguments {
                collect_expression(argument, owner, candidates, referenced);
            }
        }
        Expression::CallThrough { target, arguments } => {
            collect_expression(target, owner, candidates, referenced);
            for argument in arguments {
                collect_expression(argument, owner, candidates, referenced);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_expression(object, owner, candidates, referenced);
            for argument in arguments {
                collect_expression(argument, owner, candidates, referenced);
            }
        }
        Expression::ConstructedNew {
            constructor,
            arguments,
            ..
        } => {
            record("__nw__FUl", owner, candidates, referenced);
            record(constructor, owner, candidates, referenced);
            for argument in arguments {
                collect_expression(argument, owner, candidates, referenced);
            }
        }
        Expression::AggregateLiteral(elements) => {
            for element in elements {
                collect_expression(element, owner, candidates, referenced);
            }
        }
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            collect_expression(left, owner, candidates, referenced);
            collect_expression(right, owner, candidates, referenced);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_expression(condition, owner, candidates, referenced);
            collect_expression(when_true, owner, candidates, referenced);
            collect_expression(when_false, owner, candidates, referenced);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => collect_expression(operand, owner, candidates, referenced),
        Expression::Index { base, index } => {
            collect_expression(base, owner, candidates, referenced);
            collect_expression(index, owner, candidates, referenced);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            collect_expression(base, owner, candidates, referenced)
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_recursion_does_not_make_a_candidate_live() {
        let names = HashSet::from(["candidate".to_owned()]);
        let mut referenced = HashSet::new();
        collect_statement(
            &Statement::Expression(Expression::Call {
                name: "candidate".into(),
                arguments: Vec::new(),
            }),
            "candidate",
            &names,
            &mut referenced,
        );
        assert!(referenced.is_empty());
    }

    #[test]
    fn calls_and_address_taking_make_candidates_live() {
        let statements = [
            Statement::Expression(Expression::Call {
                name: "called".into(),
                arguments: Vec::new(),
            }),
            Statement::Expression(Expression::AddressOf {
                operand: Box::new(Expression::Variable("addressed".into())),
            }),
        ];
        let names = HashSet::from(["called".to_owned(), "addressed".to_owned()]);
        let mut referenced = HashSet::new();
        for statement in &statements {
            collect_statement(statement, "caller", &names, &mut referenced);
        }
        assert_eq!(referenced, names);
    }

    #[test]
    fn candidate_only_cycles_stay_dead_but_rooted_chains_propagate() {
        let edges = HashMap::from([
            ("a".to_owned(), HashSet::from(["b".to_owned()])),
            ("b".to_owned(), HashSet::from(["a".to_owned()])),
            ("rooted".to_owned(), HashSet::from(["leaf".to_owned()])),
        ]);
        let mut referenced = HashSet::from(["rooted".to_owned()]);
        extend_reachable_candidates(&mut referenced, &edges);
        assert_eq!(
            referenced,
            HashSet::from(["rooted".to_owned(), "leaf".to_owned()])
        );
    }

    #[test]
    fn disabled_inlining_does_not_root_a_dead_materialization_candidate() {
        let source = r#"
            void external(void);
            inline static void helper(void) { external(); }
            inline static void speculative(void) { helper(); helper(); }
            void real(void) {}
        "#;
        let unit = mwcc_tokens_to_syntax_trees::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();

        assert!(unit
            .materialized_inline_candidates
            .iter()
            .any(|name| name == "speculative"));
        assert!(unit
            .skipped_inline_definitions
            .iter()
            .any(|function| function.name == "helper"));
        assert!(referenced_disabled_inlines(&unit).is_empty());
    }
}
