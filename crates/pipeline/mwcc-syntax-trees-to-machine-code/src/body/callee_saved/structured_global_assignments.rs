//! Give static-storage assignments their memory semantics before local planning.

use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use std::collections::{HashMap, HashSet};

pub(super) fn normalize(function: &Function, globals: &HashMap<String, Type>) -> Option<Function> {
    let bindings: HashSet<&str> = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .chain(
            function
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str()),
        )
        .collect();
    let mut normalized = function.clone();
    let mut changed = false;
    for statement in &mut normalized.statements {
        super::structured_expression_visit::visit_statement_nodes_mut(
            statement,
            &mut |statement| {
                if let Statement::Assign { name, value } = statement {
                    if !bindings.contains(name.as_str()) && globals.contains_key(name) {
                        *statement = Statement::Store {
                            target: Expression::Variable(name.clone()),
                            value: value.clone(),
                        };
                        changed = true;
                    }
                }
            },
        );
    }
    changed.then_some(normalized)
}
