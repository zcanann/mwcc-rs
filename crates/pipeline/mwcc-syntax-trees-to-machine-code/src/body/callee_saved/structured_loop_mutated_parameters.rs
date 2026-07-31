//! Allocator ownership for parameters mutated inside structured loops.
//!
//! A postfix-stepped parameter must keep one home across the loop backedge.
//! Leaving the incoming ABI register pinned hides that lifetime from virtual
//! allocation; copying it to a preferred virtual makes the CFG interval
//! explicit and lets ordinary coalescing retain the ABI home when it is free.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Parameter, Statement};

use super::structured_expression_visit::visit_statement;

pub(super) fn loop_mutated_parameters(function: &Function) -> Vec<&Parameter> {
    let mut mutated = std::collections::HashSet::new();
    collect_loop_mutations(&function.statements, &mut mutated);
    function
        .parameters
        .iter()
        .filter(|parameter| mutated.contains(parameter.name.as_str()))
        .collect()
}

fn collect_loop_mutations(
    statements: &[Statement],
    mutated: &mut std::collections::HashSet<String>,
) {
    for statement in statements {
        match statement {
            Statement::Loop { .. } => visit_statement(statement, &mut |expression| {
                if let Expression::PostStep { target, .. } = expression {
                    if let Expression::Variable(name) = target.as_ref() {
                        mutated.insert(name.clone());
                    }
                }
            }),
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_loop_mutations(then_body, mutated);
                collect_loop_mutations(else_body, mutated);
            }
            Statement::Switch { arms, default, .. } => {
                for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
                    if let ArmBody::Statements(statements) = body {
                        collect_loop_mutations(statements, mutated);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::loop_mutated_parameters;
    use mwcc_syntax_trees::{
        BinaryOperator, Expression, Function, LoopKind, Parameter, Pointee, Statement, Type,
    };

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "transform".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Float),
                    name: "loop_pointer".into(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Float),
                    name: "outside_pointer".into(),
                },
            ],
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

    fn step(name: &str) -> Expression {
        Expression::PostStep {
            target: Box::new(Expression::Variable(name.into())),
            operator: BinaryOperator::Add,
            pointer_link: None,
        }
    }

    #[test]
    fn selects_only_parameters_mutated_beneath_a_loop() {
        let function = function(vec![
            Statement::Expression(step("outside_pointer")),
            Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![Statement::If {
                    condition: Expression::IntegerLiteral(1),
                    then_body: vec![Statement::Expression(step("loop_pointer"))],
                    else_body: Vec::new(),
                }],
            },
        ]);

        assert_eq!(
            loop_mutated_parameters(&function)
                .into_iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["loop_pointer"]
        );
    }
}
