use super::*;

fn assignment(name: &str, value: Expression) -> Statement {
    Statement::Assign {
        name: name.into(),
        value,
    }
}

fn complement(name: &str) -> Expression {
    Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: Box::new(Expression::FloatLiteral(1.0)),
        right: Box::new(Expression::Variable(name.into())),
    }
}

fn selection_switch() -> Statement {
    Statement::Switch {
        scrutinee: Expression::Variable("selector".into()),
        arms: ["pan", "fx", "dolby"]
            .into_iter()
            .enumerate()
            .map(|(index, source)| mwcc_syntax_trees::SwitchArm {
                value: index as i64,
                body: mwcc_syntax_trees::ArmBody::Statements(vec![assignment(
                    "angle",
                    complement(source),
                )]),
                falls_through: false,
            })
            .collect(),
        default: None,
    }
}

#[test]
fn hoists_repeated_switch_complements_and_bounds_before_the_loop() {
    let mut locals: Vec<_> = ["pan", "fx", "dolby", "angle", "tmp"]
        .into_iter()
        .map(|name| float_local(name))
        .collect();
    locals.push(LocalDeclaration {
        declared_type: Type::Int,
        ..float_local("selector")
    });
    let function = Function {
        return_type: Type::Void,
        name: "mix".into(),
        is_static: false,
        is_weak: false,
        parameters: Vec::new(),
        locals,
        statements: vec![
            assignment("pan", Expression::FloatLiteral(0.5)),
            assignment("fx", Expression::FloatLiteral(0.5)),
            assignment("dolby", Expression::FloatLiteral(0.0)),
            Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::Variable("selector".into())),
                step: None,
                body: vec![
                    selection_switch(),
                    selection_switch(),
                    Statement::Expression(Expression::Call {
                        name: "consume".into(),
                        arguments: vec![Expression::Variable("angle".into())],
                    }),
                    Statement::If {
                        condition: Expression::Binary {
                            operator: BinaryOperator::Less,
                            left: Box::new(Expression::Variable("tmp".into())),
                            right: Box::new(Expression::FloatLiteral(0.0)),
                        },
                        then_body: vec![assignment("tmp", Expression::FloatLiteral(0.0))],
                        else_body: Vec::new(),
                    },
                ],
            },
        ],
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };

    let rewritten = hoist_repeated_float_switch_invariants(&function)
        .expect("the repeated selections should be hoisted");

    assert_eq!(rewritten.locals.len(), function.locals.len() + 5);
    assert_eq!(rewritten.statements.len(), 9);
    assert!(matches!(
        &rewritten.statements[3],
        Statement::Assign {
            value: Expression::FloatLiteral(value),
            ..
        } if *value == 1.0
    ));
    assert!(matches!(
        &rewritten.statements[4],
        Statement::Assign {
            value: Expression::FloatLiteral(value),
            ..
        } if *value == 0.0
    ));
    let Statement::Loop { body, .. } = &rewritten.statements[8] else {
        panic!("the rewritten loop should follow its five invariants")
    };
    let mut remaining_complements = 0;
    for statement in body {
        super::super::structured_expression_visit::visit_statement(
            statement,
            &mut |expression| {
                remaining_complements +=
                    usize::from(one_minus_float_variable(expression).is_some());
            },
        );
    }
    assert_eq!(remaining_complements, 0);
}
