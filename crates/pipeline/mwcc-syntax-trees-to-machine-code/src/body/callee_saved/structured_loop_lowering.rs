//! Canonical CFG lowering for loops owned by the structured body emitter.
//!
//! The structured emitter already has one branch-resolution mechanism for
//! labels and gotos.  Lowering loops into that representation keeps loop
//! topology separate from statement codegen and gives nested `break` and
//! `continue` precise lexical targets.  Measured specialized loop owners stay
//! intact and can retain their instruction schedules.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) fn lower_structured_loops(
    function: &Function,
    global_array_sizes: &std::collections::HashMap<String, u32>,
    preserve_asm_tainted_for_entries: bool,
) -> Option<Function> {
    let mut lowering = LoopLowering::new(
        &function.statements,
        global_array_sizes,
        preserve_asm_tainted_for_entries,
    );
    let statements = lowering.lower_statements(&function.statements, None)?;
    lowering.changed.then(|| {
        let mut lowered = function.clone();
        lowered.statements = statements;
        lowered
    })
}

/// Remove switches that only evaluate an inert scrutinee and immediately break
/// from every arm. Keep this separate from loop lowering because liveness and
/// frame planning must see the same semantic body as the emission view.
pub(super) fn strip_side_effect_free_empty_switches(function: &Function) -> Option<Function> {
    fn strip_statements(statements: &[Statement], changed: &mut bool) -> Vec<Statement> {
        let mut stripped = Vec::with_capacity(statements.len());
        for statement in statements {
            match statement {
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } if !crate::analysis::expression_has_side_effect(scrutinee)
                    && arms
                        .iter()
                        .all(|arm| matches!(&arm.body, ArmBody::Statements(body) if body.is_empty()))
                    && default
                        .as_ref()
                        .is_none_or(|body| matches!(body, ArmBody::Statements(body) if body.is_empty())) =>
                {
                    // The scrutinee is still evaluated in C, hence the explicit
                    // side-effect proof before removing the whole statement.
                    *changed = true;
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => stripped.push(Statement::If {
                    condition: condition.clone(),
                    then_body: strip_statements(then_body, changed),
                    else_body: strip_statements(else_body, changed),
                }),
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => stripped.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body: strip_statements(body, changed),
                }),
                _ => stripped.push(statement.clone()),
            }
        }
        stripped
    }

    let mut changed = false;
    let statements = strip_statements(&function.statements, &mut changed);
    changed.then(|| {
        let mut stripped = function.clone();
        stripped.statements = statements;
        stripped
    })
}

struct LoopTargets<'a> {
    break_label: &'a str,
    continue_label: &'a str,
}

struct LoopLowering<'a> {
    global_array_sizes: &'a std::collections::HashMap<String, u32>,
    preserve_asm_tainted_for_entries: bool,
    used_labels: std::collections::HashSet<String>,
    next_loop: usize,
    changed: bool,
}

impl<'a> LoopLowering<'a> {
    fn new(
        statements: &[Statement],
        global_array_sizes: &'a std::collections::HashMap<String, u32>,
        preserve_asm_tainted_for_entries: bool,
    ) -> Self {
        let mut used_labels = std::collections::HashSet::new();
        collect_labels(statements, &mut used_labels);
        Self {
            global_array_sizes,
            preserve_asm_tainted_for_entries,
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
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    let arms = arms
                        .iter()
                        .map(|arm| {
                            let body = match &arm.body {
                                ArmBody::Statements(statements) => {
                                    ArmBody::Statements(
                                        self.lower_statements(statements, targets)?,
                                    )
                                }
                                ArmBody::Return(value) => {
                                    ArmBody::Return(value.clone())
                                }
                            };
                            Some(mwcc_syntax_trees::SwitchArm {
                                value: arm.value,
                                body,
                                falls_through: arm.falls_through,
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let default = match default {
                        Some(ArmBody::Statements(statements)) => {
                            Some(ArmBody::Statements(
                                self.lower_statements(statements, targets)?,
                            ))
                        }
                        Some(ArmBody::Return(value)) => {
                            Some(ArmBody::Return(value.clone()))
                        }
                        None => None,
                    };
                    lowered.push(Statement::Switch {
                        scrutinee: scrutinee.clone(),
                        arms,
                        default,
                    });
                }
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
        // Macro padding commonly leaves `do { } while (0)` in the semantic
        // tree. It has no runtime effect, but its paired source local still
        // contributes to frame layout, so remove only the control-flow shell.
        if kind == LoopKind::DoWhile
            && body.is_empty()
            && condition.and_then(constant_value) == Some(0)
        {
            self.changed = true;
            return Some(());
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
            push_effect_expressions(initializer, output);
        }
        let needs_entry_test = kind != LoopKind::DoWhile
            && condition
                .and_then(constant_value)
                .is_none_or(|value| value == 0);
        if kind == LoopKind::For && self.preserve_asm_tainted_for_entries {
            // GC/1.2.5's optimizer keeps two otherwise-empty entry labels
            // after an earlier asm function. A counted loop whose initializer
            // proves its first test true enters the body through a third label;
            // other for-loops retain their ordinary jump to the condition.
            for _ in 0..2 {
                let padding = self.fresh_label("asm_entry");
                output.push(Statement::Goto(padding.clone()));
                output.push(Statement::Label(padding));
            }
            if first_iteration_is_proven(initializer, condition) {
                output.push(Statement::Goto(body_label.clone()));
            } else if needs_entry_test {
                output.push(Statement::Goto(condition_label.clone()));
            }
            // An always-true pre-test loop enters its body directly. Retaining the
            // generic jump-to-condition creates an otherwise dead entry trampoline
            // before polling loops such as `while (1) { if (done) break; }`.
        } else if needs_entry_test && !first_iteration_is_proven(initializer, condition) {
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
            match constant_value(condition) {
                Some(0) => {}
                Some(_) => output.push(Statement::Goto(body_label)),
                None => output.push(Statement::If {
                    condition: condition.clone(),
                    then_body: vec![Statement::Goto(body_label)],
                    else_body: Vec::new(),
                }),
            }
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

fn first_iteration_is_proven(
    initializer: Option<&Expression>,
    condition: Option<&Expression>,
) -> bool {
    let Some(Expression::Binary {
        operator,
        left,
        right,
    }) = condition
    else {
        return false;
    };
    let Expression::Variable(name) = left.as_ref() else {
        return false;
    };
    let Some(left) = initializer.and_then(|initializer| assigned_constant(initializer, name)) else {
        return false;
    };
    let Some(right) = constant_value(right) else {
        return false;
    };
    match operator {
        BinaryOperator::Equal => left == right,
        BinaryOperator::NotEqual => left != right,
        BinaryOperator::Less => left < right,
        BinaryOperator::LessEqual => left <= right,
        BinaryOperator::Greater => left > right,
        BinaryOperator::GreaterEqual => left >= right,
        _ => false,
    }
}

fn assigned_constant(expression: &Expression, name: &str) -> Option<i64> {
    match expression {
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(target) if target == name) =>
        {
            constant_value(value)
        }
        Expression::Comma { left, right } => assigned_constant(right, name)
            .or_else(|| assigned_constant(left, name)),
        _ => None,
    }
}

/// A comma in a value-discarded for-clause is an ordered statement sequence.
/// Keeping each assignment visible lets the structured emitter retain normal
/// local-definition handling while preserving the source evaluation order.
fn push_effect_expressions(expression: &Expression, output: &mut Vec<Statement>) {
    if let Expression::Comma { left, right } = expression {
        push_effect_expressions(left, output);
        push_effect_expressions(right, output);
    } else if let Expression::Assign { target, value } = expression {
        if let Expression::Variable(name) = target.as_ref() {
            output.push(Statement::Assign {
                name: name.clone(),
                value: value.as_ref().clone(),
            });
        } else {
            output.push(Statement::Expression(expression.clone()));
        }
    } else {
        output.push(Statement::Expression(expression.clone()));
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
            Statement::Switch { arms, default, .. } => {
                for arm in arms {
                    if let ArmBody::Statements(body) = &arm.body {
                        collect_labels(body, labels);
                    }
                }
                if let Some(ArmBody::Statements(body)) = default {
                    collect_labels(body, labels);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::SwitchArm;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "structured".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

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

        let lowered = lower_structured_loops(&function, &Default::default(), false)
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

    #[test]
    fn preserves_three_asm_tainted_counted_loop_entry_edges() {
        let counted = Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(16)),
            }),
            step: None,
            body: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("i".into())],
            })],
        };
        let lowered = lower_structured_loops(
            &function(vec![counted]),
            &Default::default(),
            true,
        )
        .expect("the counted loop should lower");

        assert!(matches!(
            lowered.statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Goto(first),
                Statement::Label(first_label),
                Statement::Goto(second),
                Statement::Label(second_label),
                Statement::Goto(body),
                Statement::Label(body_label),
                ..
            ] if name == "i"
                && first == first_label
                && second == second_label
                && body == body_label
        ));
    }

    #[test]
    fn lowers_loops_retained_inside_switch_arms() {
        let mut function = function(vec![Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![SwitchArm {
                value: 0xdcd1_0000,
                body: ArmBody::Statements(vec![Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::Variable("pending".into())),
                    step: None,
                    body: vec![Statement::Expression(Expression::Call {
                        name: "poll".into(),
                        arguments: Vec::new(),
                    })],
                }]),
                falls_through: false,
            }],
            default: None,
        }]);
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "kind".into(),
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

        let lowered = lower_structured_loops(&function, &Default::default(), false)
            .expect("a loop inside a retained switch should lower");
        let Statement::Switch { arms, .. } = &lowered.statements[0] else {
            panic!("the source switch should remain visible");
        };
        let ArmBody::Statements(body) = &arms[0].body else {
            panic!("the arm should retain its statement body");
        };
        assert!(!body
            .iter()
            .any(|statement| matches!(statement, Statement::Loop { .. })));
        assert!(body
            .iter()
            .any(|statement| matches!(statement, Statement::Label(_))));
    }

    #[test]
    fn removes_a_side_effect_free_switch_with_only_empty_break_arms() {
        let function = Function {
            return_type: Type::Void,
            name: "wait".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Switch {
                scrutinee: Expression::Variable("state".into()),
                arms: vec![
                    SwitchArm {
                        value: -1,
                        body: ArmBody::Statements(Vec::new()),
                        falls_through: false,
                    },
                    SwitchArm {
                        value: 1,
                        body: ArmBody::Statements(Vec::new()),
                        falls_through: false,
                    },
                ],
                default: None,
            }],
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

        let lowered = strip_side_effect_free_empty_switches(&function)
            .expect("empty break-only switch should be removed");

        assert!(lowered.statements.is_empty());
    }

    #[test]
    fn retains_an_empty_switch_with_a_side_effecting_scrutinee() {
        let function = Function {
            return_type: Type::Void,
            name: "wait".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Switch {
                scrutinee: Expression::Call {
                    name: "status".into(),
                    arguments: Vec::new(),
                },
                arms: vec![SwitchArm {
                    value: 0,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: false,
                }],
                default: None,
            }],
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

        assert!(strip_side_effect_free_empty_switches(&function).is_none());
    }

    #[test]
    fn removes_an_empty_false_do_while_shell() {
        let mut function = Function {
            return_type: Type::Void,
            name: "padding".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Loop {
                kind: LoopKind::DoWhile,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(0)),
                step: None,
                body: Vec::new(),
            }],
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

        function = lower_structured_loops(&function, &Default::default(), false)
            .expect("empty false do-while should be removed");

        assert!(function.statements.is_empty());
    }

    #[test]
    fn lowers_a_nonempty_false_do_while_to_one_body_execution() {
        let mut function = Function {
            return_type: Type::Void,
            name: "macro_shell".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Loop {
                kind: LoopKind::DoWhile,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(0)),
                step: None,
                body: vec![Statement::Expression(Expression::Call {
                    name: "sink".into(),
                    arguments: Vec::new(),
                })],
            }],
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

        function = lower_structured_loops(&function, &Default::default(), false)
            .expect("nonempty false do-while should lower");

        assert_eq!(
            function
                .statements
                .iter()
                .filter(|statement| matches!(statement, Statement::Expression(_)))
                .count(),
            1
        );
        assert!(!function
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::If { .. })));
        assert_eq!(
            super::structured::structured_hidden_label_count(&function.statements),
            4
        );
    }
}
