use super::*;

fn var(name: &str) -> E {
    E::Variable(name.into())
}
fn number(value: u32) -> E {
    E::IntegerLiteral(i64::from(value))
}
fn binary(operator: B, left: E, right: E) -> E {
    E::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn word() -> E {
    E::Index {
        base: Box::new(var("words")),
        index: Box::new(number(1)),
    }
}
fn mask(bits: u32) -> E {
    binary(B::BitAnd, word(), number(bits))
}
fn store(target: E, value: E) -> S {
    S::Store { target, value }
}
fn call(name: &str, arguments: Vec<E>) -> S {
    S::Expression(E::Call {
        name: name.into(),
        arguments,
    })
}
fn local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::UnsignedInt,
        name: name.into(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: vec![],
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    }
}
fn fixture() -> Function {
    let mut packet = local("words");
    packet.array_length = Some(2);
    Function {
        name: "inspect".into(),
        return_type: Type::Void,
        is_static: true,
        is_weak: false,
        parameters: vec![],
        locals: vec![packet],
        statements: vec![
            call("state", vec![var("words")]),
            S::If {
                condition: mask(4),
                else_body: vec![],
                then_body: vec![
                    call("receive", vec![var("words")]),
                    store(word(), mask(0x7fffffff)),
                    S::If {
                        condition: binary(B::Equal, mask(0x0f000000), number(0x0f000000)),
                        else_body: vec![],
                        then_body: vec![
                            store(var("message"), word()),
                            store(var("count"), mask(0x3fff)),
                            store(var("ready"), number(7)),
                        ],
                    },
                ],
            },
        ],
        guards: vec![],
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: vec![],
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}
fn guarded_query() -> Function {
    let mut f = fixture();
    f.name = "query".into();
    f.return_type = Type::Int;
    f.locals.push(local("token"));
    let mut transaction = vec![S::Assign {
        name: "token".into(),
        value: E::Call {
            name: "acquire".into(),
            arguments: vec![],
        },
    }];
    transaction.append(&mut f.statements);
    f.statements = vec![
        store(var("ready"), number(0)),
        S::If {
            condition: binary(B::Equal, var("count"), number(0)),
            then_body: transaction,
            else_body: vec![],
        },
        call("release", vec![var("token")]),
    ];
    f.return_expression = Some(var("count"));
    f
}

#[test]
fn captures_packet_fields_and_preserves_partial_token_definition() {
    let f = fixture();
    let p = helper(&f).unwrap();
    assert_eq!(
        (
            p.index,
            p.ready,
            p.preserve,
            p.tag_mask,
            p.tag,
            p.field_mask,
            p.flag_value
        ),
        (1, 4, 0x7fffffff, 0x0f000000, 0x0f000000, 0x3fff, 7)
    );
    let f = guarded_query();
    let q = query(&f).unwrap();
    assert_eq!(
        (q.acquire, q.release, q.token),
        ("acquire", "release", "token")
    );
    // Do not repair the source's missing initialization on the bypass path.
    assert!(f.locals[1].initializer.is_none());
}

#[test]
fn rejects_unmodeled_storage_effects_and_packet_bindings() {
    for change in 0..13 {
        let mut f = fixture();
        match change {
            0 => f.locals[0].is_volatile = true,
            1 => f.locals[0].is_static = true,
            2 => f.locals[0].initializer = Some(number(0)),
            3 => f.locals[0].declared_type = Type::Int,
            4 => f.locals[0].array_length = Some(1),
            5 => f.locals[0].attribute_alignment = Some(32),
            6 => f.locals[0].row_bytes = Some(8),
            7 => f.statements.push(call("extra", vec![])),
            8 => f.statements[0] = call("state", vec![var("other")]),
            9 => f.return_expression = Some(number(0)),
            10..=12 => {
                let S::If {
                    condition,
                    then_body,
                    ..
                } = &mut f.statements[1]
                else {
                    unreachable!()
                };
                match change {
                    10 => *condition = binary(B::BitAnd, word(), var("runtime_mask")),
                    11 => {
                        then_body[1] = store(
                            word(),
                            binary(
                                B::BitAnd,
                                E::Index {
                                    base: Box::new(var("words")),
                                    index: Box::new(number(0)),
                                },
                                number(0x7fffffff),
                            ),
                        )
                    }
                    _ => then_body.push(S::Return(None)),
                }
            }
            _ => unreachable!(),
        }
        assert!(helper(&f).is_none(), "negative {change}");
    }
    let mut f = fixture();
    let S::If { then_body, .. } = &mut f.statements[1] else {
        unreachable!()
    };
    let S::If { then_body, .. } = &mut then_body[2] else {
        unreachable!()
    };
    then_body[1] = store(var("message"), mask(0x3fff));
    assert!(helper(&f).is_none(), "aliased publications");
}

#[test]
fn query_declines_token_changes_and_wrong_publication_identity() {
    for change in 0..6 {
        let mut f = guarded_query();
        match change {
            0 => f.locals[1].initializer = Some(number(0)),
            1 => f.locals[1].is_volatile = true,
            2 => f.statements[2] = call("release", vec![number(0)]),
            3 => f.return_expression = Some(var("message")),
            4 => f.statements[0] = store(var("other"), number(0)),
            _ => {
                let S::If { else_body, .. } = &mut f.statements[1] else {
                    unreachable!()
                };
                else_body.push(call("extra", vec![]));
            }
        }
        assert!(query(&f).is_none(), "query negative {change}");
    }
}

#[test]
fn automatic_composition_renames_storage_and_declines_global_capture() {
    let helper_body = fixture();
    let mut caller = fixture();
    caller.name = "caller".into();
    caller.locals.clear();
    caller.statements = vec![call("inspect", vec![])];
    let bodies = crate::inline_expansion::InlineBodySet::analyze_with_definitions(
        &[helper_body.clone(), caller.clone()],
        &[],
    );
    let expanded = bodies.expand_calls(&caller).unwrap();
    assert_ne!(expanded.locals[0].name, "words");
    assert!(helper(&expanded).is_some());
    for name in ["message", "count", "ready", "state", "receive"] {
        let mut shadowed = caller.clone();
        let mut shadow = local(name);
        shadow.initializer = Some(number(0));
        shadowed.locals.push(shadow);
        assert!(bodies.expand_calls(&shadowed).is_none(), "capture {name}");
    }
}
