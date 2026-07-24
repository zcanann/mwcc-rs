//! Semantics-preserving loop normalization shared by body owners.
//!
//! Macro wrappers frequently leave `do { ... } while (0)` in the AST. Their
//! body executes once, so owners that otherwise handle straight-line code
//! should not each need a private loop special case.

use super::*;

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
            _ => output.push(statement.clone()),
        }
    }
    (output, changed)
}

/// A break/continue nested in an `if` targets this loop; one inside a nested
/// loop does not. Leave the former to CFG lowering until the transform can
/// preserve its early-exit behavior.
fn has_direct_loop_control(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Break | Statement::Continue => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => has_direct_loop_control(then_body) || has_direct_loop_control(else_body),
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

        let normalized =
            flatten_constant_false_do_while(&function).expect("shell should flatten");

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
}
