use super::*;
use std::collections::HashMap;

fn var(name: &str) -> Expression {
    Expression::Variable(name.into())
}
fn integer(value: i64) -> Expression {
    Expression::IntegerLiteral(value)
}
fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
    Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn bank(index: i64) -> Expression {
    Expression::Index {
        base: Box::new(var("bank")),
        index: Box::new(integer(index)),
    }
}
fn assign(name: &str, value: Expression) -> Statement {
    Statement::Assign {
        name: name.into(),
        value,
    }
}
fn local(name: &str, declared_type: Type, initializer: Option<Expression>) -> LocalDeclaration {
    LocalDeclaration {
        name: name.into(),
        declared_type,
        initializer,
        is_volatile: false,
        is_static: false,
        array_length: None,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    }
}
fn transfer(data: Expression, length: i64, mode: i64) -> Statement {
    assign(
        "error",
        binary(
            BinaryOperator::BitOr,
            var("error"),
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: Box::new(Expression::Call {
                    name: "exchange".into(),
                    arguments: vec![data, integer(length), integer(mode)],
                }),
            },
        ),
    )
}
fn wait() -> Statement {
    Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        step: None,
        body: Vec::new(),
        condition: Some(binary(BinaryOperator::BitAnd, bank(5), integer(4))),
    }
}
fn fixture(read: bool) -> Function {
    let payload = if read {
        integer(0x20000000)
    } else {
        binary(
            BinaryOperator::BitOr,
            binary(BinaryOperator::BitAnd, var("value"), integer(0x0fffffff)),
            integer(0x80000000),
        )
    };
    let mut statements = vec![
        assign(
            "selected",
            binary(BinaryOperator::BitAnd, var("selected"), integer(0x811)),
        ),
        assign(
            "selected",
            binary(BinaryOperator::BitOr, var("selected"), integer(0x20)),
        ),
        Statement::Store {
            target: bank(3),
            value: var("selected"),
        },
        assign("payload", payload),
        transfer(
            Expression::AddressOf {
                operand: Box::new(var("payload")),
            },
            if read { 2 } else { 4 },
            1,
        ),
        wait(),
    ];
    if read {
        statements.extend([transfer(var("value"), 4, 0), wait()]);
    }
    statements.push(Statement::Store {
        target: bank(3),
        value: Expression::IndexedUpdateValue {
            value: Box::new(binary(BinaryOperator::BitAnd, bank(3), integer(0x811))),
        },
    });
    Function {
        name: "transaction".into(),
        return_type: Type::Int,
        is_static: false,
        is_weak: false,
        parameters: vec![mwcc_syntax_trees::Parameter {
            name: "value".into(),
            parameter_type: if read {
                Type::Pointer(Pointee::UnsignedInt)
            } else {
                Type::UnsignedInt
            },
        }],
        locals: vec![
            local("error", Type::Int, Some(integer(0))),
            local("payload", Type::UnsignedInt, None),
            local("selected", Type::UnsignedInt, Some(bank(3))),
        ],
        statements,
        guards: Vec::new(),
        return_expression: Some(Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(var("error")),
        }),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}
fn banks() -> HashMap<String, (u32, Type)> {
    HashMap::from([("bank".into(), (0xcc00f000, Type::UnsignedInt))])
}

#[test]
fn derives_addresses_masks_payload_and_calls_from_the_tree() {
    let function = fixture(false);
    let plan = recognize::transaction(&function, &banks()).unwrap();
    assert_eq!(
        (plan.address, plan.selected, plan.poll),
        (0xcc00f000, 12, 20)
    );
    assert_eq!(
        (plan.preserve, plan.insert, plan.poll_begin, plan.poll_end),
        (0x811, 0x20, 29, 29)
    );
    assert_eq!(plan.transfer, "exchange");
    assert!(matches!(
        plan.payload,
        Payload::Write {
            begin: 4,
            end: 31,
            high: 0x8000
        }
    ));
    assert!(matches!(
        recognize::transaction(&fixture(true), &banks())
            .unwrap()
            .payload,
        Payload::Read { high: 0x2000 }
    ));
}

#[test]
fn refuses_a_poll_with_effects_or_a_changed_second_poll() {
    let mut effectful = fixture(false);
    let Statement::Loop { body, .. } = &mut effectful.statements[5] else {
        unreachable!()
    };
    body.push(Statement::Expression(Expression::Call {
        name: "observe".into(),
        arguments: Vec::new(),
    }));
    let mut changed = fixture(true);
    let Statement::Loop { condition, .. } = &mut changed.statements[7] else {
        unreachable!()
    };
    *condition = Some(binary(BinaryOperator::BitAnd, bank(6), integer(4)));
    for function in [effectful, changed] {
        assert!(recognize::transaction(&function, &banks()).is_none());
    }
}

#[test]
fn refuses_a_different_reset_slot_mask_or_narrowing_cast() {
    for (target, value) in [
        (
            bank(4),
            binary(BinaryOperator::BitAnd, bank(4), integer(0x811)),
        ),
        (
            bank(3),
            binary(BinaryOperator::BitAnd, bank(3), integer(0x810)),
        ),
        (
            bank(3),
            Expression::Cast {
                target_type: Type::UnsignedChar,
                operand: Box::new(binary(BinaryOperator::BitAnd, bank(3), integer(0x811))),
            },
        ),
    ] {
        let mut function = fixture(false);
        *function.statements.last_mut().unwrap() = Statement::Store {
            target,
            value: Expression::IndexedUpdateValue {
                value: Box::new(value),
            },
        };
        assert!(recognize::transaction(&function, &banks()).is_none());
    }
}

#[test]
fn refuses_changed_transfer_arguments_and_pointer_truncation() {
    for (data, length, mode) in [
        (
            Expression::AddressOf {
                operand: Box::new(var("payload")),
            },
            3,
            1,
        ),
        (
            Expression::AddressOf {
                operand: Box::new(var("payload")),
            },
            4,
            0,
        ),
        (
            Expression::Cast {
                target_type: Type::UnsignedChar,
                operand: Box::new(Expression::AddressOf {
                    operand: Box::new(var("payload")),
                }),
            },
            4,
            1,
        ),
    ] {
        let mut function = fixture(false);
        function.statements[4] = transfer(data, length, mode);
        assert!(recognize::transaction(&function, &banks()).is_none());
    }
}

#[test]
fn refuses_observable_or_additional_locals_and_unknown_banks() {
    let mut volatile = fixture(false);
    volatile.locals[1].is_volatile = true;
    let mut persistent = fixture(false);
    persistent.locals[1].is_static = true;
    let mut initialized = fixture(false);
    initialized.locals[1].initializer = Some(integer(7));
    let mut aligned = fixture(false);
    aligned.locals[1].attribute_alignment = Some(32);
    let mut extra = fixture(false);
    extra
        .locals
        .push(local("extra", Type::Int, Some(integer(1))));
    for function in [volatile, persistent, initialized, aligned, extra] {
        assert!(recognize::transaction(&function, &banks()).is_none());
    }
    assert!(recognize::transaction(&fixture(false), &HashMap::new()).is_none());
}

#[test]
fn algebraic_constant_identities_do_not_hide_volatile_reads() {
    let mut function = fixture(false);
    function.statements[0] = assign(
        "selected",
        binary(
            BinaryOperator::BitAnd,
            var("selected"),
            binary(
                BinaryOperator::Add,
                integer(0x811),
                binary(BinaryOperator::Subtract, bank(4), bank(4)),
            ),
        ),
    );
    assert!(recognize::transaction(&function, &banks()).is_none());
}
