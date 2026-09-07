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

fn stream_fixture(writing: bool) -> Function {
    let mut f = fixture(false);
    f.parameters.extend([
        mwcc_syntax_trees::Parameter {
            name: "data".into(),
            parameter_type: Type::Pointer(Pointee::UnsignedInt),
        },
        mwcc_syntax_trees::Parameter {
            name: "count".into(),
            parameter_type: Type::Int,
        },
    ]);
    f.locals.extend([
        local(
            "cursor",
            Type::Pointer(Pointee::UnsignedInt),
            Some(var("data")),
        ),
        local("word", Type::UnsignedInt, None),
    ]);
    f.statements[3] = assign(
        "payload",
        binary(
            BinaryOperator::BitOr,
            binary(
                BinaryOperator::ShiftLeft,
                binary(BinaryOperator::BitAnd, var("value"), integer(0x1fffc0)),
                integer(6),
            ),
            integer(0x80000000),
        ),
    );
    let address = Expression::Dereference {
        pointer: Box::new(Expression::PostStep {
            target: Box::new(var("cursor")),
            operator: BinaryOperator::Add,
            pointer_link: None,
        }),
    };
    let exchange = transfer(
        Expression::AddressOf {
            operand: Box::new(var("word")),
        },
        4,
        i64::from(writing),
    );
    let mut body = if writing {
        vec![assign("word", address), exchange, wait()]
    } else {
        vec![
            exchange,
            wait(),
            Statement::Store {
                target: address,
                value: var("word"),
            },
        ]
    };
    body.extend([
        assign(
            "count",
            binary(BinaryOperator::Subtract, var("count"), integer(4)),
        ),
        Statement::If {
            condition: binary(BinaryOperator::Less, var("count"), integer(0)),
            then_body: vec![assign("count", integer(0))],
            else_body: Vec::new(),
        },
    ]);
    f.statements.insert(
        6,
        Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            step: None,
            condition: Some(binary(BinaryOperator::NotEqual, var("count"), integer(0))),
            body,
        },
    );
    f
}

#[test]
fn stream_recognition_derives_both_directions_and_command_geometry() {
    for writing in [false, true] {
        let f = stream_fixture(writing);
        let plan = stream::transaction(&f, &banks()).unwrap();
        assert_eq!(
            (plan.address, plan.selected, plan.poll),
            (0xcc00f000, 12, 20)
        );
        assert!(
            matches!(plan.payload, Payload::Stream { writing: direction, command }
            if direction == writing && command.shift == 6 && command.mask == (11, 25)
                && command.fused_mask == (5, 19) && !command.shift_first && command.high == 0x8000)
        );
    }
}

#[test]
fn stream_rejects_extra_effects_and_changed_bounds_or_clamps() {
    for writing in [false, true] {
        for change in 0..5 {
            let mut f = stream_fixture(writing);
            let Statement::Loop {
                condition, body, ..
            } = &mut f.statements[6]
            else {
                unreachable!()
            };
            match change {
                0 => body.push(assign("error", integer(0))),
                1 => *condition = Some(binary(BinaryOperator::Greater, var("count"), integer(0))),
                2 => {
                    body[3] = assign(
                        "count",
                        binary(BinaryOperator::Subtract, var("count"), integer(1)),
                    )
                }
                3 => {
                    let Statement::If { then_body, .. } = &mut body[4] else {
                        unreachable!()
                    };
                    then_body[0] = assign("count", integer(4));
                }
                _ => {
                    let Statement::If { else_body, .. } = &mut body[4] else {
                        unreachable!()
                    };
                    else_body.push(assign("error", integer(0)));
                }
            }
            assert!(stream::transaction(&f, &banks()).is_none());
        }
    }
}

#[test]
fn stream_requires_unsigned_word_storage_and_distinct_roles() {
    for change in 0..5 {
        let mut f = stream_fixture(false);
        match change {
            0 => f.locals[3].declared_type = Type::Pointer(Pointee::UnsignedChar),
            1 => f.locals[4].declared_type = Type::Int,
            2 => f.locals[3].is_volatile = true,
            3 => f.locals[4].name = "cursor".into(),
            _ => f.parameters[2].parameter_type = Type::UnsignedInt,
        }
        assert!(stream::transaction(&f, &banks()).is_none());
    }
}

#[test]
fn stream_rejects_mismatched_poll_and_nonconstant_command_masks() {
    let mut f = stream_fixture(false);
    let Statement::Loop { body, .. } = &mut f.statements[6] else {
        unreachable!()
    };
    let Statement::Loop { condition, .. } = &mut body[1] else {
        unreachable!()
    };
    *condition = Some(binary(BinaryOperator::BitAnd, bank(7), integer(4)));
    assert!(stream::transaction(&f, &banks()).is_none());
    let mut f = stream_fixture(true);
    f.statements[3] = assign(
        "payload",
        binary(
            BinaryOperator::BitOr,
            binary(
                BinaryOperator::ShiftLeft,
                binary(
                    BinaryOperator::BitAnd,
                    var("value"),
                    binary(BinaryOperator::Subtract, bank(1), bank(1)),
                ),
                integer(6),
            ),
            integer(0x80000000),
        ),
    );
    assert!(stream::transaction(&f, &banks()).is_none());
}

#[test]
fn composed_packet_reads_require_one_consistent_retained_bank() {
    let first_function = fixture(true);
    let second_function = fixture(true);
    let bank_map = banks();
    let first = recognize::transaction(&first_function, &bank_map).unwrap();
    let mut second = recognize::transaction(&second_function, &bank_map).unwrap();
    assert!(packet_reads::shared_read_bank(&first, &second));
    // Commands and transfer symbols have independent owners; the retained
    // register-bank state alone must agree across both transactions.
    second.payload = Payload::Read { high: 0x8000 };
    second.transfer = "other_exchange";
    assert!(packet_reads::shared_read_bank(&first, &second));
    for changed in 0..7 {
        let mut second = recognize::transaction(&second_function, &bank_map).unwrap();
        match changed {
            0 => second.address ^= 0x10000,
            1 => second.selected += 4,
            2 => second.poll += 4,
            3 => second.preserve ^= 1,
            4 => second.insert ^= 2,
            5 => second.poll_begin -= 1,
            _ => second.poll_end += 1,
        }
        assert!(
            !packet_reads::shared_read_bank(&first, &second),
            "bank component {changed}"
        );
    }
}

#[test]
fn fixed_bank_read_does_not_capture_a_parameter_binding() {
    let mut function = fixture(true);
    function.parameters[0].name = "bank".into();
    function.statements[6] = transfer(var("bank"), 4, 0);
    assert!(recognize::transaction(&function, &banks()).is_none());
}
