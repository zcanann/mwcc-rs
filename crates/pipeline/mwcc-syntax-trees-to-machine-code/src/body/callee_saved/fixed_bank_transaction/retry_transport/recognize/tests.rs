use super::*;
use mwcc_syntax_trees::{ConditionalOrigin, Parameter};

fn var(name: &str) -> Expression {
    Expression::Variable(name.into())
}
fn int(value: i64) -> Expression {
    Expression::IntegerLiteral(value)
}
fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
    Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn call(name: &str, arguments: Vec<Expression>) -> Expression {
    Expression::Call {
        name: name.into(),
        arguments,
    }
}
fn assign(name: &str, value: Expression) -> Statement {
    Statement::Assign {
        name: name.into(),
        value,
    }
}
fn retry(name: &str, arguments: Vec<Expression>) -> Statement {
    Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        step: None,
        condition: Some(Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(call(name, arguments)),
        }),
        body: vec![],
    }
}
fn poll(checked: bool) -> Statement {
    let args = vec![Expression::AddressOf {
        operand: Box::new(var("busy")),
    }];
    Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        step: None,
        condition: Some(binary(BinaryOperator::BitAnd, var("busy"), int(8))),
        body: vec![if checked {
            retry("state", args)
        } else {
            Statement::Expression(call("state", args))
        }],
    }
}
fn fixture() -> Function {
    use BinaryOperator::*;
    Function {
        name: "send".into(),
        return_type: Type::Int,
        is_static: false,
        is_weak: false,
        parameters: vec![
            Parameter {
                name: "buffer".into(),
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
            },
            Parameter {
                name: "length".into(),
                parameter_type: Type::UnsignedInt,
            },
        ],
        locals: [
            ("token", Type::Int),
            ("busy", Type::UnsignedInt),
            ("value", Type::UnsignedInt),
        ]
        .into_iter()
        .map(|(name, declared_type)| LocalDeclaration {
            name: name.into(),
            declared_type,
            initializer: None,
            is_volatile: false,
            is_static: false,
            array_length: None,
            data_bytes: None,
            data_relocations: vec![],
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        })
        .collect(),
        statements: vec![
            assign("token", call("acquire", vec![])),
            poll(false),
            Statement::Store {
                target: var("sequence"),
                value: binary(Add, var("sequence"), int(1)),
            },
            assign(
                "value",
                Expression::Conditional {
                    condition: Box::new(binary(BitAnd, var("sequence"), int(4))),
                    when_true: Box::new(int(0x800)),
                    when_false: Box::new(int(0)),
                    origin: ConditionalOrigin::Ternary,
                },
            ),
            retry(
                "stream",
                vec![
                    binary(BitOr, var("value"), int(0x34000)),
                    var("buffer"),
                    binary(
                        BitAnd,
                        binary(Subtract, binary(Add, var("length"), int(8)), int(1)),
                        int(-8),
                    ),
                ],
            ),
            poll(false),
            assign(
                "value",
                binary(
                    BitOr,
                    binary(
                        BitOr,
                        binary(ShiftLeft, var("sequence"), int(8)),
                        int(0x07000000),
                    ),
                    var("length"),
                ),
            ),
            retry("message", vec![var("value")]),
            poll(true),
            Statement::Expression(call("release", vec![var("token")])),
        ],
        guards: vec![],
        return_expression: Some(int(-7)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: vec![],
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}

#[test]
fn derives_transport_roles_and_command_geometry() {
    let f = fixture();
    let plan = transport(&f).unwrap();
    assert_eq!(
        (
            plan.acquire,
            plan.release,
            plan.status,
            plan.stream,
            plan.mailbox,
            plan.counter
        ),
        ("acquire", "release", "state", "stream", "message", "sequence")
    );
    assert_eq!((plan.busy_mask, plan.counter_mask), ((28, 28), (29, 29)));
    assert_eq!(
        (
            plan.offset,
            plan.stream_base,
            plan.rounding_bias,
            plan.rounding_mask
        ),
        (0x800, 0x34000, 7, (0, 28))
    );
    assert_eq!(
        (plan.sequence_shift, plan.message_tag, plan.result),
        (8, 0x700, -7)
    );
}

#[test]
fn requires_the_distinct_failure_policy_of_each_status_phase() {
    for index in [1, 5, 8] {
        let mut f = fixture();
        f.statements[index] = poll(index != 8);
        assert!(transport(&f).is_none(), "phase {index}");
    }
    for index in [1, 5, 8] {
        for change in 0..3 {
            let mut f = fixture();
            let Statement::Loop {
                condition, body, ..
            } = &mut f.statements[index]
            else {
                unreachable!()
            };
            match change {
                0 => *condition = Some(binary(BinaryOperator::BitAnd, var("busy"), int(4))),
                1 => body.push(Statement::Expression(call("observe", vec![]))),
                _ => {
                    body[0] = Statement::Expression(call(
                        "other_state",
                        vec![Expression::AddressOf {
                            operand: Box::new(var("busy")),
                        }],
                    ))
                }
            }
            assert!(transport(&f).is_none(), "phase {index}, change {change}");
        }
    }
}

#[test]
fn rejects_effectful_retries_rounding_changes_and_pointer_truncation() {
    for index in [4, 7] {
        let mut f = fixture();
        let Statement::Loop { body, .. } = &mut f.statements[index] else {
            unreachable!()
        };
        body.push(assign("value", int(0)));
        assert!(transport(&f).is_none());
    }
    for change in 0..5 {
        let mut f = fixture();
        let Statement::Loop {
            condition: Some(Expression::Unary { operand, .. }),
            ..
        } = &mut f.statements[4]
        else {
            unreachable!()
        };
        let Expression::Call { arguments, .. } = operand.as_mut() else {
            unreachable!()
        };
        match change {
            0 => {
                arguments[1] = Expression::Cast {
                    target_type: Type::UnsignedChar,
                    operand: Box::new(var("buffer")),
                }
            }
            1 => {
                arguments[2] = binary(
                    BinaryOperator::BitAnd,
                    binary(BinaryOperator::Add, var("length"), int(6)),
                    int(-7),
                )
            }
            2 => {
                arguments[2] = binary(
                    BinaryOperator::BitAnd,
                    binary(BinaryOperator::Add, var("length"), int(7)),
                    int(-4),
                )
            }
            3 => {
                arguments[2] = binary(
                    BinaryOperator::BitAnd,
                    binary(BinaryOperator::Add, var("length"), call("bias", vec![])),
                    int(-8),
                )
            }
            _ => arguments[0] = binary(BinaryOperator::BitOr, var("value"), int(0x30000)),
        }
        assert!(transport(&f).is_none(), "change {change}");
    }
}

#[test]
fn rejects_captured_bindings_observable_storage_and_changed_token() {
    for change in 0..8 {
        let mut f = fixture();
        match change {
            0 => f.locals[0].is_volatile = true,
            1 => f.locals[1].initializer = Some(int(0)),
            2 => f.locals[2].is_static = true,
            3 => f.locals[2].name = "stream".into(),
            4 => f.parameters[0].name = "sequence".into(),
            5 => f.statements[9] = Statement::Expression(call("release", vec![var("value")])),
            6 => {
                f.statements[2] = Statement::Store {
                    target: var("sequence"),
                    value: binary(BinaryOperator::Add, var("sequence"), int(2)),
                }
            }
            _ => f.return_expression = Some(call("finish", vec![])),
        }
        assert!(transport(&f).is_none(), "change {change}");
    }
}
