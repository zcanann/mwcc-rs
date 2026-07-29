//! Path-sensitive interference for deferred callee-saved locals.
//!
//! The ordinary home planner keeps MWCC's source-order allocation preferences,
//! but flattened source intervals overstate pressure across mutually exclusive
//! arms. This module supplies the control-flow proof needed to reuse a home in
//! that case. Unsupported non-forward control flow declines conservatively.

use super::*;
use super::structured_locals::expression_assignment_count;

pub(super) struct DeferredInterference<'a> {
    names: std::collections::HashSet<&'a str>,
    edges: std::collections::HashSet<(&'a str, &'a str)>,
}

impl<'a> DeferredInterference<'a> {
    pub(super) fn analyze(function: &'a Function, names: &[&'a str]) -> Option<Self> {
        let names = names.iter().copied().collect();
        let mut analysis = Self {
            names,
            edges: std::collections::HashSet::new(),
        };
        let mut live = analysis.reads(function.return_expression.as_ref());
        analysis.add_clique(&live);
        live = analysis.statements(&function.statements, live)?;
        analysis.add_clique(&live);
        Some(analysis)
    }

    pub(super) fn interferes(&self, left: &str, right: &str) -> bool {
        left == right || self.edges.contains(&(left, right)) || self.edges.contains(&(right, left))
    }

    fn statements(
        &mut self,
        statements: &'a [Statement],
        mut live: std::collections::HashSet<&'a str>,
    ) -> Option<std::collections::HashSet<&'a str>> {
        for statement in statements.iter().rev() {
            self.add_clique(&live);
            match statement {
                Statement::Assign { name, value } => {
                    if self.expression_assigns_selected(value) {
                        return None;
                    }
                    if let Some(name) = self.names.get(name.as_str()).copied() {
                        for other in &live {
                            if *other != name {
                                self.edges.insert((name, *other));
                            }
                        }
                        live.remove(name);
                    }
                    live.extend(self.reads(Some(value)));
                }
                Statement::Store { target, value } => {
                    if self.expression_assigns_selected(target)
                        || self.expression_assigns_selected(value)
                    {
                        return None;
                    }
                    live.extend(self.reads(Some(target)));
                    live.extend(self.reads(Some(value)));
                }
                Statement::Expression(expression) => {
                    if self.expression_assigns_selected(expression) {
                        return None;
                    }
                    live.extend(self.reads(Some(expression)));
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    if self.expression_assigns_selected(condition) {
                        return None;
                    }
                    let then_live = self.statements(then_body, live.clone())?;
                    let else_live = self.statements(else_body, live.clone())?;
                    live = then_live.union(&else_live).copied().collect();
                    live.extend(self.reads(Some(condition)));
                }
                Statement::Return(expression) => {
                    if expression
                        .as_ref()
                        .is_some_and(|value| self.expression_assigns_selected(value))
                    {
                        return None;
                    }
                    live = self.reads(expression.as_ref());
                }
                Statement::InlineAsm(_)
                | Statement::Switch { .. }
                | Statement::Break
                | Statement::Continue
                | Statement::Goto(_)
                | Statement::Label(_)
                | Statement::Loop { .. } => return None,
            }
            self.add_clique(&live);
        }
        Some(live)
    }

    fn reads(&self, expression: Option<&Expression>) -> std::collections::HashSet<&'a str> {
        self.names
            .iter()
            .copied()
            .filter(|name| {
                expression.is_some_and(|expression| expression_reads_name(expression, name))
            })
            .collect()
    }

    fn expression_assigns_selected(&self, expression: &Expression) -> bool {
        self.names
            .iter()
            .any(|name| expression_assignment_count(expression, name) != 0)
    }

    fn add_clique(&mut self, live: &std::collections::HashSet<&'a str>) {
        for (offset, left) in live.iter().enumerate() {
            for right in live.iter().skip(offset + 1) {
                self.edges.insert((*left, *right));
            }
        }
    }
}
