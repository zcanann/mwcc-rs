use super::*;
use mwcc_syntax_trees::Parameter;
use std::collections::HashMap;
use BinaryOperator::*;

fn v(n: &str) -> Expression {
    Expression::Variable(n.into())
}
fn c(n: i64) -> Expression {
    Expression::IntegerLiteral(n)
}
fn b(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
    Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn assign(name: &str, value: Expression) -> Statement {
    Statement::Assign {
        name: name.into(),
        value,
    }
}
fn slot(n: i64) -> Expression {
    Expression::Index {
        base: Box::new(v("bank")),
        index: Box::new(c(n)),
    }
}
fn ptr() -> Expression {
    Expression::Cast {
        target_type: Type::Pointer(Pointee::UnsignedChar),
        operand: Box::new(v("buffer")),
    }
}
fn shift() -> Expression {
    b(ShiftLeft, b(Subtract, c(3), v("i")), c(3))
}
fn counted(body: Vec<Statement>) -> Statement {
    Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Assign {
            target: Box::new(v("i")),
            value: Box::new(c(0)),
        }),
        condition: Some(b(Less, v("i"), v("length"))),
        step: Some(Expression::Assign {
            target: Box::new(v("i")),
            value: Box::new(b(Add, v("i"), c(1))),
        }),
        body,
    }
}
fn fixture() -> Function {
    let locals = [
        ("packed", Type::UnsignedInt),
        ("received", Type::UnsignedInt),
        ("i", Type::Int),
        ("source", Type::Pointer(Pointee::UnsignedChar)),
        ("destination", Type::Pointer(Pointee::UnsignedChar)),
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
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    })
    .collect();
    Function {
        name: "arbitrary_transfer_name".into(),
        return_type: Type::Int,
        is_static: false,
        is_weak: false,
        parameters: [
            ("buffer", Type::Pointer(Pointee::UnsignedChar)),
            ("length", Type::Int),
            ("mode", Type::UnsignedInt),
        ]
        .into_iter()
        .map(|(name, parameter_type)| Parameter {
            name: name.into(),
            parameter_type,
        })
        .collect(),
        locals,
        statements: vec![
            Statement::If {
                condition: v("mode"),
                else_body: vec![],
                then_body: vec![
                    assign("packed", c(0)),
                    counted(vec![
                        assign("source", b(Add, ptr(), v("i"))),
                        assign(
                            "packed",
                            b(
                                BitOr,
                                v("packed"),
                                b(
                                    ShiftLeft,
                                    Expression::Dereference {
                                        pointer: Box::new(v("source")),
                                    },
                                    shift(),
                                ),
                            ),
                        ),
                    ]),
                    Statement::Store {
                        target: slot(11),
                        value: v("packed"),
                    },
                ],
            },
            Statement::Store {
                target: slot(10),
                value: b(
                    BitOr,
                    b(BitOr, c(4), b(ShiftLeft, v("mode"), c(3))),
                    b(ShiftLeft, b(Subtract, v("length"), c(1)), c(5)),
                ),
            },
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(b(BitAnd, slot(10), c(4))),
                step: None,
                body: vec![],
            },
            Statement::If {
                condition: Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: Box::new(v("mode")),
                },
                else_body: vec![],
                then_body: vec![
                    assign("destination", ptr()),
                    assign("received", slot(11)),
                    counted(vec![Statement::Store {
                        target: Expression::Dereference {
                            pointer: Box::new(Expression::PostStep {
                                target: Box::new(v("destination")),
                                operator: Add,
                                pointer_link: None,
                            }),
                        },
                        value: b(ShiftRight, v("received"), shift()),
                    }]),
                ],
            },
        ],
        guards: vec![],
        return_expression: Some(c(1)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: vec![],
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}
fn banks() -> HashMap<String, (u32, Type)> {
    HashMap::from([("bank".into(), (0xcc00f000, Type::UnsignedInt))])
}

#[test]
fn derives_the_device_fields_and_source_loop_names() {
    let function = fixture();
    let banks = banks();
    let p = recognize::transfer(&function, &banks).unwrap();
    assert_eq!(
        (p.address, p.data_offset, p.control_offset),
        (0xcc00f000, 44, 40)
    );
    assert_eq!(
        (p.start, p.mode_shift, p.count_shift, p.poll_bit),
        (4, 3, 5, 29)
    );
    assert_eq!((p.index, p.count), ("i", "length"));
}
#[test]
fn extra_effects_in_either_loop_or_poll_are_not_dropped() {
    for arm in [0, 2, 3] {
        let mut f = fixture();
        match &mut f.statements[arm] {
            Statement::If { then_body, .. } => {
                if let Some(Statement::Loop { body, .. }) = then_body
                    .iter_mut()
                    .find(|s| matches!(s, Statement::Loop { .. }))
                {
                    body.push(assign("packed", c(9)));
                }
            }
            Statement::Loop { body, .. } => body.push(assign("packed", c(9))),
            _ => unreachable!(),
        }
        assert!(recognize::transfer(&f, &banks()).is_none());
    }
}
#[test]
fn storage_qualifiers_and_signed_bytes_do_not_enter_the_unsigned_packet() {
    for index in 0..5 {
        let mut f = fixture();
        f.locals[index].is_volatile = true;
        assert!(recognize::transfer(&f, &banks()).is_none());
    }
    let mut f = fixture();
    f.locals[3].declared_type = Type::Pointer(Pointee::Char);
    assert!(recognize::transfer(&f, &banks()).is_none());
    let mut f = fixture();
    f.locals[1].declared_type = Type::Int;
    assert!(recognize::transfer(&f, &banks()).is_none());
    let mut f = fixture();
    f.parameters[1].parameter_type = Type::UnsignedInt;
    assert!(recognize::transfer(&f, &banks()).is_none());
}
#[test]
fn changed_bounds_and_steps_do_not_reuse_the_counted_packet() {
    for change in 0..3 {
        let mut f = fixture();
        let Statement::If { then_body, .. } = &mut f.statements[0] else {
            unreachable!()
        };
        let Statement::Loop {
            initializer,
            condition,
            step,
            ..
        } = &mut then_body[1]
        else {
            unreachable!()
        };
        match change {
            0 => {
                *initializer = Some(Expression::Assign {
                    target: Box::new(v("i")),
                    value: Box::new(c(1)),
                })
            }
            1 => *condition = Some(b(LessEqual, v("i"), v("length"))),
            _ => {
                *step = Some(Expression::Assign {
                    target: Box::new(v("i")),
                    value: Box::new(b(Add, v("i"), c(2))),
                })
            }
        }
        assert!(recognize::transfer(&f, &banks()).is_none());
    }
}
#[test]
fn different_read_registers_and_nonword_banks_are_rejected() {
    let mut f = fixture();
    let Statement::If { then_body, .. } = &mut f.statements[3] else {
        unreachable!()
    };
    let Statement::Assign { value, .. } = &mut then_body[1] else {
        unreachable!()
    };
    *value = slot(12);
    assert!(recognize::transfer(&f, &banks()).is_none());
    let mut wrong = banks();
    wrong.get_mut("bank").unwrap().1 = Type::UnsignedShort;
    assert!(recognize::transfer(&fixture(), &wrong).is_none());
}
#[test]
fn a_runtime_identity_is_not_a_device_index_constant() {
    let mut f = fixture();
    let Statement::Store { target, .. } = &mut f.statements[1] else {
        unreachable!()
    };
    let Expression::Index { index, .. } = target else {
        unreachable!()
    };
    *index = Box::new(b(Add, c(10), b(Subtract, slot(2), slot(2))));
    assert!(recognize::transfer(&f, &banks()).is_none());
}
#[test]
fn narrowing_pointer_casts_and_aliasing_local_roles_are_rejected() {
    let mut f = fixture();
    let Statement::If { then_body, .. } = &mut f.statements[3] else {
        unreachable!()
    };
    let Statement::Assign { value, .. } = &mut then_body[0] else {
        unreachable!()
    };
    let Expression::Cast { operand, .. } = value else {
        unreachable!()
    };
    *operand = Box::new(Expression::Cast {
        target_type: Type::UnsignedShort,
        operand: Box::new(v("buffer")),
    });
    assert!(recognize::transfer(&f, &banks()).is_none());
    let mut f = fixture();
    f.locals[4].name = "source".into();
    assert!(recognize::transfer(&f, &banks()).is_none());
}
