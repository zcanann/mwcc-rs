//! Semantics-preserving loop normalization shared by body owners.
//!
//! Macro wrappers frequently leave `do { ... } while (0)` in the AST. Their
//! body executes once, so owners that otherwise handle straight-line code
//! should not each need a private loop special case.

use super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) fn flatten_constant_false_do_while(function: &Function) -> Option<Function> {
    let (statements, changed) = flatten_statements(&function.statements);
    changed.then(|| {
        let mut normalized = function.clone();
        normalized.statements = statements;
        normalized
    })
}

fn flatten_statements(statements: &[Statement]) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(statements.len());
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Loop {
                kind: LoopKind::DoWhile,
                initializer: None,
                condition: Some(condition),
                step: None,
                body,
            } if constant_value(condition) == Some(0) && !has_direct_loop_control(body) => {
                let (body, _) = flatten_statements(body);
                output.extend(body);
                changed = true;
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let (then_body, then_changed) = flatten_statements(then_body);
                let (else_body, else_changed) = flatten_statements(else_body);
                output.push(Statement::If {
                    condition: condition.clone(),
                    then_body,
                    else_body,
                });
                changed |= then_changed || else_changed;
            }
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => {
                let (body, body_changed) = flatten_statements(body);
                output.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body,
                });
                changed |= body_changed;
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                let arms = arms
                    .iter()
                    .map(|arm| {
                        let (body, body_changed) = flatten_arm(&arm.body);
                        changed |= body_changed;
                        mwcc_syntax_trees::SwitchArm {
                            value: arm.value,
                            body,
                            falls_through: arm.falls_through,
                        }
                    })
                    .collect();
                let default = default.as_ref().map(|body| {
                    let (body, body_changed) = flatten_arm(body);
                    changed |= body_changed;
                    body
                });
                output.push(Statement::Switch {
                    scrutinee: scrutinee.clone(),
                    arms,
                    default,
                });
            }
            _ => output.push(statement.clone()),
        }
    }
    (output, changed)
}

fn flatten_arm(body: &ArmBody) -> (ArmBody, bool) {
    match body {
        ArmBody::Statements(statements) => {
            let (statements, changed) = flatten_statements(statements);
            (ArmBody::Statements(statements), changed)
        }
        ArmBody::Return(_) => (body.clone(), false),
    }
}

/// A switch captures breaks, but its continues still target the surrounding
/// loop. A nested loop captures both. Keep wrappers with their own early exits
/// until CFG lowering can preserve those edges.
fn has_direct_loop_control(statements: &[Statement]) -> bool {
    has_loop_control(statements, true)
}

fn has_loop_control(statements: &[Statement], break_targets_loop: bool) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Break => break_targets_loop,
        Statement::Continue => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            has_loop_control(then_body, break_targets_loop)
                || has_loop_control(else_body, break_targets_loop)
        }
        Statement::Switch { arms, default, .. } => {
            let escapes = |body: &ArmBody| match body {
                ArmBody::Statements(statements) => has_loop_control(statements, false),
                ArmBody::Return(_) => false,
            };
            arms.iter().any(|arm| escapes(&arm.body)) || default.as_ref().is_some_and(escapes)
        }
        Statement::Loop { .. } => false,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function_with(statement: Statement) -> Function {
        Function {
            return_type: Type::Void,
            name: "wrapper".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![statement],
            return_expression: None,
            guards: Vec::new(),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn flattens_a_constant_false_do_while_body_once() {
        let statement = Statement::Expression(Expression::Call {
            name: "sink".into(),
            arguments: Vec::new(),
        });
        let function = function_with(Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(0)),
            step: None,
            body: vec![statement.clone()],
        });

        let normalized = flatten_constant_false_do_while(&function).expect("shell should flatten");

        assert!(matches!(
            normalized.statements.as_slice(),
            [Statement::Expression(Expression::Call { name, arguments })]
                if name == "sink" && arguments.is_empty()
        ));
    }

    #[test]
    fn leaves_direct_break_semantics_for_cfg_lowering() {
        let function = function_with(Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(0)),
            step: None,
            body: vec![Statement::Break],
        });

        assert!(flatten_constant_false_do_while(&function).is_none());
    }

    fn wrapper(body: Vec<Statement>) -> Statement {
        Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(0)),
            step: None,
            body,
        }
    }

    fn switch(body: Vec<Statement>, default: Option<ArmBody>) -> Statement {
        Statement::Switch {
            scrutinee: Expression::Variable("selector".into()),
            arms: vec![mwcc_syntax_trees::SwitchArm {
                value: 7,
                body: ArmBody::Statements(body),
                falls_through: true,
            }],
            default,
        }
    }

    #[test]
    fn flattens_case_and_default_wrappers_without_changing_fallthrough() {
        let function = function_with(switch(
            vec![wrapper(vec![Statement::Return(None)])],
            Some(ArmBody::Statements(vec![wrapper(vec![Statement::Return(
                None,
            )])])),
        ));
        let normalized = flatten_constant_false_do_while(&function).unwrap();
        let Statement::Switch { arms, default, .. } = &normalized.statements[0] else {
            panic!("switch must remain");
        };
        assert_eq!(arms[0].value, 7);
        assert!(arms[0].falls_through);
        assert!(matches!(&arms[0].body,
            ArmBody::Statements(body) if matches!(body.as_slice(), [Statement::Return(None)])));
        assert!(matches!(default,
            Some(ArmBody::Statements(body)) if matches!(body.as_slice(), [Statement::Return(None)])));
        assert!(flatten_constant_false_do_while(&normalized).is_none());
    }

    #[test]
    fn switch_continue_keeps_the_surrounding_wrapper() {
        for body in [
            switch(vec![Statement::Continue], None),
            switch(
                Vec::new(),
                Some(ArmBody::Statements(vec![Statement::Continue])),
            ),
        ] {
            let function = function_with(wrapper(vec![body]));
            assert!(flatten_constant_false_do_while(&function).is_none());
        }
    }

    #[test]
    fn switch_break_and_nested_loop_continue_do_not_escape_the_wrapper() {
        for body in [
            switch(vec![Statement::Break], None),
            wrapper(vec![Statement::Continue]),
        ] {
            let function = function_with(wrapper(vec![body]));
            let normalized = flatten_constant_false_do_while(&function).unwrap();
            assert_eq!(normalized.statements.len(), 1);
            assert!(flatten_constant_false_do_while(&normalized).is_none());
        }
    }
}
