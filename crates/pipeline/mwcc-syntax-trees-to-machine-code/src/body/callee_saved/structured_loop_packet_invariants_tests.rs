use super::*;

fn local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::UnsignedInt,
        name: name.into(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    }
}

fn packet_word(base: &str, offset: u32, value: Expression) -> Statement {
    Statement::Store {
        target: Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        },
        value,
    }
}

fn repeated_value() -> Expression {
    let mut value = Expression::Variable("a".into());
    for constant in 1..=8 {
        value = Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: Box::new(value),
            right: Box::new(Expression::IntegerLiteral(constant)),
        };
    }
    value
}

fn function(statements: Vec<Statement>) -> Function {
    Function {
        return_type: Type::Void,
        name: "packets".into(),
        is_static: false,
        is_weak: false,
        parameters: Vec::new(),
        locals: vec![local("a"), local("cursor")],
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

fn loop_with(body: Vec<Statement>) -> Statement {
    Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(Expression::Variable("a".into())),
        step: None,
        body,
    }
}

#[test]
fn hoists_a_repeated_pure_packet_word_before_its_loop() {
    let function = function(vec![
        Statement::Assign {
            name: "a".into(),
            value: Expression::IntegerLiteral(4),
        },
        loop_with(vec![
            packet_word("cursor", 0, repeated_value()),
            packet_word("cursor", 8, repeated_value()),
        ]),
    ]);
    let hoisted = hoist_repeated_packet_words(&function).expect("hoisted packet word");
    assert_eq!(hoisted.locals.len(), 3);
    assert!(matches!(
        &hoisted.statements[1],
        Statement::Assign { name, .. } if name == "__mwcc_packet_word_0"
    ));
    let Statement::Loop { body, .. } = &hoisted.statements[2] else {
        panic!("expected loop")
    };
    assert!(body.iter().all(|statement| matches!(
        statement,
        Statement::Store {
            value: Expression::Variable(name),
            ..
        } if name == "__mwcc_packet_word_0"
    )));
}

#[test]
fn does_not_hoist_a_value_whose_input_is_written_by_the_loop() {
    let function = function(vec![
        Statement::Assign {
            name: "a".into(),
            value: Expression::IntegerLiteral(4),
        },
        loop_with(vec![
            packet_word("cursor", 0, repeated_value()),
            Statement::Assign {
                name: "a".into(),
                value: Expression::IntegerLiteral(5),
            },
            packet_word("cursor", 8, repeated_value()),
        ]),
    ]);
    assert!(hoist_repeated_packet_words(&function).is_none());
}

#[test]
fn accepts_a_value_assigned_on_both_if_branches() {
    let function = function(vec![
        Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![Statement::Assign {
                name: "a".into(),
                value: Expression::IntegerLiteral(4),
            }],
            else_body: vec![Statement::Assign {
                name: "a".into(),
                value: Expression::IntegerLiteral(5),
            }],
        },
        loop_with(vec![
            packet_word("cursor", 0, repeated_value()),
            packet_word("cursor", 8, repeated_value()),
        ]),
    ]);
    assert!(hoist_repeated_packet_words(&function).is_some());
}

#[test]
fn does_not_hoist_an_address_taken_input() {
    let function = function(vec![
        Statement::Assign {
            name: "a".into(),
            value: Expression::IntegerLiteral(4),
        },
        Statement::Expression(Expression::AddressOf {
            operand: Box::new(Expression::Variable("a".into())),
        }),
        loop_with(vec![
            packet_word("cursor", 0, repeated_value()),
            packet_word("cursor", 8, repeated_value()),
        ]),
    ]);
    assert!(hoist_repeated_packet_words(&function).is_none());
}

#[test]
fn does_not_combine_single_uses_from_separate_loops() {
    let function = function(vec![
        Statement::Assign {
            name: "a".into(),
            value: Expression::IntegerLiteral(4),
        },
        loop_with(vec![packet_word("cursor", 0, repeated_value())]),
        loop_with(vec![packet_word("cursor", 8, repeated_value())]),
    ]);
    assert!(hoist_repeated_packet_words(&function).is_none());
}
