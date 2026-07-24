//! Canonical CFG lowering for loops owned by the structured body emitter.
//!
//! The structured emitter already has one branch-resolution mechanism for
//! labels and gotos.  Lowering loops into that representation keeps loop
//! topology separate from statement codegen and gives nested `break` and
//! `continue` precise lexical targets.  Measured specialized loop owners stay
//! intact and can retain their instruction schedules.

#[allow(unused_imports)]
use super::*;

pub(super) fn lower_structured_loops(
    function: &Function,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<Function> {
    let mut lowering = LoopLowering::new(&function.statements, global_array_sizes);
    let statements = lowering.lower_statements(&function.statements, None)?;
    lowering.changed.then(|| {
        let mut lowered = function.clone();
        lowered.statements = statements;
        lowered
    })
}

struct LoopTargets<'a> {
    break_label: &'a str,
    continue_label: &'a str,
}

struct LoopLowering<'a> {
    global_array_sizes: &'a std::collections::HashMap<String, u32>,
    used_labels: std::collections::HashSet<String>,
    next_loop: usize,
    changed: bool,
}

impl<'a> LoopLowering<'a> {
    fn new(
        statements: &[Statement],
        global_array_sizes: &'a std::collections::HashMap<String, u32>,
    ) -> Self {
        let mut used_labels = std::collections::HashSet::new();
        collect_labels(statements, &mut used_labels);
        Self {
            global_array_sizes,
            used_labels,
            next_loop: 0,
            changed: false,
        }
    }

    fn lower_statements(
        &mut self,
        statements: &[Statement],
        targets: Option<&LoopTargets<'_>>,
    ) -> Option<Vec<Statement>> {
        let mut lowered = Vec::new();
        for statement in statements {
            match statement {
                Statement::Loop { .. }
                    if super::super::global_struct_member_search::is_global_struct_member_search_loop(
                        statement,
                        self.global_array_sizes,
                    ) => lowered.push(statement.clone()),
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => self.lower_loop(
                    *kind,
                    initializer.as_ref(),
                    condition.as_ref(),
                    step.as_ref(),
                    body,
                    &mut lowered,
                )?,
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => lowered.push(Statement::If {
                    condition: condition.clone(),
                    then_body: self.lower_statements(then_body, targets)?,
                    else_body: self.lower_statements(else_body, targets)?,
                }),
                Statement::Break => lowered.push(Statement::Goto(
                    targets?.break_label.to_owned(),
                )),
                Statement::Continue => lowered.push(Statement::Goto(
                    targets?.continue_label.to_owned(),
                )),
                _ => lowered.push(statement.clone()),
            }
        }
        Some(lowered)
    }

    fn lower_loop(
        &mut self,
        kind: LoopKind,
        initializer: Option<&Expression>,
        condition: Option<&Expression>,
        step: Option<&Expression>,
        body: &[Statement],
        output: &mut Vec<Statement>,
    ) -> Option<()> {
        if kind != LoopKind::For && (initializer.is_some() || step.is_some()) {
            return None;
        }
        self.changed = true;
        let body_label = self.fresh_label("body");
        let continue_label = self.fresh_label("continue");
        let condition_label = self.fresh_label("condition");
        let exit_label = self.fresh_label("exit");
        let targets = LoopTargets {
            break_label: &exit_label,
            continue_label: &continue_label,
        };
        let body = self.lower_statements(body, Some(&targets))?;

        if let Some(initializer) = initializer {
            output.push(Statement::Expression(initializer.clone()));
        }
        if kind != LoopKind::DoWhile {
            output.push(Statement::Goto(condition_label.clone()));
        }
        output.push(Statement::Label(body_label.clone()));
        output.extend(body);
        output.push(Statement::Label(continue_label));
        if let Some(step) = step {
            output.push(Statement::Expression(step.clone()));
        }
        output.push(Statement::Label(condition_label));
        if let Some(condition) = condition {
            output.push(Statement::If {
                condition: condition.clone(),
                then_body: vec![Statement::Goto(body_label)],
                else_body: Vec::new(),
            });
        } else {
            output.push(Statement::Goto(body_label));
        }
        output.push(Statement::Label(exit_label));
        Some(())
    }

    fn fresh_label(&mut self, role: &str) -> String {
        loop {
            let label = format!("__mwcc_structured_loop_{}_{}", self.next_loop, role);
            self.next_loop += 1;
            if self.used_labels.insert(label.clone()) {
                return label;
            }
        }
    }
}

fn collect_labels(statements: &[Statement], labels: &mut std::collections::HashSet<String>) {
    for statement in statements {
        match statement {
            Statement::Label(label) => {
                labels.insert(label.clone());
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_labels(then_body, labels);
                collect_labels(else_body, labels);
            }
            Statement::Loop { body, .. } => collect_labels(body, labels),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn for_loop(body: Vec<Statement>) -> Statement {
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("cursor".into())),
                value: Box::new(Expression::Variable("head".into())),
            }),
            condition: Some(Expression::Variable("cursor".into())),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("cursor".into())),
                value: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("cursor".into())),
                    offset: 8,
                    member_type: Type::StructPointer { element_size: 0 },
                    index_stride: None,
                }),
            }),
            body,
        }
    }

    #[test]
    fn lowers_continue_and_break_to_distinct_loop_labels() {
        let function = Function {
            return_type: Type::Void,
            name: "walk".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![for_loop(vec![
                Statement::If {
                    condition: Expression::Variable("skip".into()),
                    then_body: vec![Statement::Continue],
                    else_body: Vec::new(),
                },
                Statement::Break,
            ])],
            return_expression: None,
            guards: Vec::new(),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let lowered = lower_structured_loops(&function, &Default::default())
            .expect("ordinary loop should lower");

        assert!(lowered.statements.iter().any(|statement| matches!(
            statement,
            Statement::If { then_body, .. }
                if matches!(then_body.as_slice(), [Statement::Goto(label)]
                    if label.contains("continue"))
        )));
        assert!(lowered.statements.iter().any(|statement| matches!(
            statement,
            Statement::Goto(label) if label.contains("exit")
        )));
        assert!(!lowered
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::Loop { .. })));
    }
}
