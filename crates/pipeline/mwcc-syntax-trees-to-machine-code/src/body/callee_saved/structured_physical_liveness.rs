//! Named-value liveness for selection's remaining physical homes.
//!
//! Machine liveness cannot repair a scratch instruction that already overwrote
//! a named physical home. Analyze the structured emitter's label/if graph before
//! selecting those instructions, including enclosing continuations and backedges.

use super::structured_liveness::statement_reads_name;
use crate::analysis::expression_reads_name;
use mwcc_syntax_trees::{Expression, Function, Statement};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Node<'a> {
    statement: Option<&'a Statement>,
    uses: HashSet<&'a str>,
    definition: Option<&'a str>,
    successors: Vec<usize>,
}

pub(super) struct PhysicalHomeLiveness<'a> {
    nodes: Vec<Node<'a>>,
    live_out: Vec<HashSet<&'a str>>,
}

impl<'a> PhysicalHomeLiveness<'a> {
    pub(super) fn new(function: &'a Function) -> Self {
        let names: Vec<_> = function
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .chain(function.locals.iter().map(|l| l.name.as_str()))
            .collect();
        Self::analyze(
            &function.statements,
            function.return_expression.as_ref(),
            &names,
        )
    }

    fn analyze(statements: &'a [Statement], tail: Option<&Expression>, names: &[&'a str]) -> Self {
        let reads = |expression: &Expression| {
            names
                .iter()
                .copied()
                .filter(|name| expression_reads_name(expression, name))
                .collect()
        };
        let mut nodes = vec![Node {
            uses: tail.map(reads).unwrap_or_default(),
            ..Node::default()
        }];
        let mut labels = HashMap::new();
        build(statements, 0, names, &mut nodes, &mut labels);
        for node in &mut nodes {
            if let Some(Statement::Goto(label)) = node.statement {
                if let Some(&target) = labels.get(label.as_str()) {
                    node.successors.push(target);
                } else {
                    // An unknown target is rejected by emission. Until then,
                    // it cannot establish that a physical home is dead.
                    node.uses.extend(names.iter().copied());
                }
            }
        }
        let mut live_in = vec![HashSet::new(); nodes.len()];
        let mut live_out = live_in.clone();
        loop {
            let mut changed = false;
            for (index, node) in nodes.iter().enumerate() {
                let out: HashSet<_> = node
                    .successors
                    .iter()
                    .flat_map(|&next| live_in[next].iter().copied())
                    .collect();
                let mut input = out.clone();
                if let Some(name) = node.definition {
                    input.remove(name);
                }
                input.extend(node.uses.iter().copied());
                changed |= input != live_in[index] || out != live_out[index];
                live_in[index] = input;
                live_out[index] = out;
            }
            if !changed {
                break;
            }
        }
        Self { nodes, live_out }
    }

    pub(super) fn after(&self, statement: &Statement) -> Option<&HashSet<&'a str>> {
        // Identity is scoped to this borrowed emission tree. Specialized owners
        // that synthesize their own statements retain their existing policies.
        self.nodes
            .iter()
            .position(|node| {
                node.statement
                    .is_some_and(|source| std::ptr::eq(source, statement))
            })
            .map(|index| &self.live_out[index])
    }
}

fn build<'a>(
    statements: &'a [Statement],
    mut next: usize,
    names: &[&'a str],
    nodes: &mut Vec<Node<'a>>,
    labels: &mut HashMap<&'a str, usize>,
) -> usize {
    for statement in statements.iter().rev() {
        let mut node = Node {
            statement: Some(statement),
            successors: vec![next],
            ..Node::default()
        };
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                node.uses.extend(
                    names
                        .iter()
                        .copied()
                        .filter(|name| expression_reads_name(condition, name)),
                );
                node.successors = vec![
                    build(then_body, next, names, nodes, labels),
                    build(else_body, next, names, nodes, labels),
                ];
            }
            Statement::Assign { name, value } => {
                node.definition = Some(name);
                node.uses.extend(
                    names
                        .iter()
                        .copied()
                        .filter(|name| expression_reads_name(value, name)),
                );
            }
            Statement::Goto(_) => node.successors.clear(),
            Statement::Return(_) => {
                node.successors.clear();
                node.uses.extend(
                    names
                        .iter()
                        .copied()
                        .filter(|name| statement_reads_name(statement, name)),
                );
            }
            Statement::Label(label) => {
                labels.insert(label, nodes.len());
            }
            Statement::InlineAsm(_) | Statement::Break | Statement::Continue => {
                node.uses.extend(names.iter().copied());
            }
            // Ordinary loops have already become labels and gotos. Retained
            // loop/switch owners are opaque here: count all their reads and
            // infer no kills, preserving the incoming homes conservatively.
            _ => node.uses.extend(
                names
                    .iter()
                    .copied()
                    .filter(|name| statement_reads_name(statement, name)),
            ),
        }
        next = nodes.len();
        nodes.push(node);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> Statement {
        Statement::Expression(Expression::Variable(name.into()))
    }
    fn set(name: &str) -> Statement {
        Statement::Assign {
            name: name.into(),
            value: Expression::IntegerLiteral(0),
        }
    }
    fn branch(then_body: Vec<Statement>, else_body: Vec<Statement>) -> Statement {
        Statement::If {
            condition: Expression::Variable("mode".into()),
            then_body,
            else_body,
        }
    }
    fn live<'a>(statements: &'a [Statement]) -> PhysicalHomeLiveness<'a> {
        PhysicalHomeLiveness::analyze(statements, None, &["data", "mode", "result"])
    }

    #[test]
    fn an_arm_store_preserves_a_pointer_used_after_the_join() {
        let statements = [branch(vec![set("result")], vec![]), read("data")];
        let Statement::If { then_body, .. } = &statements[0] else {
            unreachable!()
        };
        assert!(live(&statements)
            .after(&then_body[0])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn a_later_branch_body_keeps_a_pointer_live_before_its_condition() {
        let statements = [set("result"), branch(vec![read("data")], vec![])];
        assert!(live(&statements)
            .after(&statements[0])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn a_backedge_keeps_a_value_live_after_its_textual_read() {
        let statements = [
            Statement::Label("head".into()),
            read("data"),
            set("result"),
            branch(vec![Statement::Goto("head".into())], vec![]),
        ];
        assert!(live(&statements)
            .after(&statements[2])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn redefinition_on_every_arm_ends_the_incoming_lifetime() {
        let statements = [
            set("result"),
            branch(vec![set("data")], vec![set("data")]),
            read("data"),
        ];
        assert!(!live(&statements)
            .after(&statements[0])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn a_redefinition_on_one_arm_keeps_the_other_incoming_lifetime() {
        let statements = [
            set("result"),
            branch(vec![set("data")], vec![]),
            read("data"),
        ];
        assert!(live(&statements)
            .after(&statements[0])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn returns_and_gotos_do_not_acquire_textually_later_reads() {
        let statements = [
            set("result"),
            Statement::Goto("done".into()),
            read("data"),
            Statement::Label("done".into()),
            Statement::Return(None),
            read("data"),
        ];
        assert!(!live(&statements)
            .after(&statements[0])
            .unwrap()
            .contains("data"));
    }

    #[test]
    fn the_fallthrough_return_expression_keeps_its_input_live() {
        let statements = [set("result")];
        let tail = Expression::Variable("data".into());
        let plan = PhysicalHomeLiveness::analyze(&statements, Some(&tail), &["data", "result"]);
        assert!(plan.after(&statements[0]).unwrap().contains("data"));
    }
}
