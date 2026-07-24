//! Large assertion-string high halves retained across structured loops.

#[allow(unused_imports)]
use super::*;

pub(super) struct LoopAssertionStrings {
    pub(super) callee: String,
    pub(super) file: Vec<u8>,
    pub(super) asserted: Vec<u8>,
}

pub(super) fn plan_loop_assertion_strings(function: &Function) -> Option<LoopAssertionStrings> {
    let mut found = None;
    collect(&function.statements, false, &mut found)?;
    found
}

fn collect(
    statements: &[Statement],
    inside_loop: bool,
    found: &mut Option<LoopAssertionStrings>,
) -> Option<()> {
    for statement in statements {
        match statement {
            Statement::Expression(expression) if inside_loop => {
                let expression = match expression {
                    Expression::Comma { left, .. } => left.as_ref(),
                    expression => expression,
                };
                let Some((callee, arguments)) =
                    super::super::assertion_expression::simple_discarded_assertion_call(expression)
                else {
                    continue;
                };
                let [
                    Expression::StringLiteral(file),
                    _,
                    Expression::StringLiteral(asserted),
                ] = arguments
                else {
                    continue;
                };
                if file.len() + 1 <= 8 || asserted.len() + 1 <= 8 || found.is_some() {
                    return None;
                }
                *found = Some(LoopAssertionStrings {
                    callee: callee.to_owned(),
                    file: file.clone(),
                    asserted: asserted.clone(),
                });
            }
            Statement::Loop { body, .. } => collect(body, true, found)?,
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect(then_body, inside_loop, found)?;
                collect(else_body, inside_loop, found)?;
            }
            _ => {}
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "walk".into(),
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

    #[test]
    fn ignores_large_strings_outside_a_loop() {
        let report = Statement::Expression(Expression::StringLiteral(vec![b'x'; 16]));
        let function = function(vec![report]);
        assert!(plan_loop_assertion_strings(&function).is_none());
    }

    #[test]
    fn recognizes_a_comma_wrapped_loop_assertion() {
        let file = b"LinkList.h".to_vec();
        let asserted = b"NW4HBM::Pointer must not be NULL (p)".to_vec();
        let report = Expression::Call {
            name: "Panic".into(),
            arguments: vec![
                Expression::StringLiteral(file.clone()),
                Expression::IntegerLiteral(573),
                Expression::StringLiteral(asserted.clone()),
            ],
        };
        let assertion = Expression::Cast {
            target_type: Type::Void,
            operand: Box::new(Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: Box::new(Expression::Variable("pointer".into())),
                right: Box::new(Expression::Comma {
                    left: Box::new(report),
                    right: Box::new(Expression::IntegerLiteral(0)),
                }),
            }),
        };
        let statement = Statement::Expression(Expression::Comma {
            left: Box::new(assertion),
            right: Box::new(Expression::VirtualCall {
                object: Box::new(Expression::Variable("pointer".into())),
                vptr_offset: 0,
                slot_offset: 88,
                return_type: Type::Void,
                variadic: false,
                arguments: Vec::new(),
            }),
        });
        let function = function(vec![Statement::Loop {
            kind: LoopKind::For,
            initializer: None,
            condition: None,
            step: None,
            body: vec![statement],
        }]);

        let plan = plan_loop_assertion_strings(&function).expect("loop assertion plan");
        assert_eq!(plan.callee, "Panic");
        assert_eq!(plan.file, file);
        assert_eq!(plan.asserted, asserted);
    }
}
