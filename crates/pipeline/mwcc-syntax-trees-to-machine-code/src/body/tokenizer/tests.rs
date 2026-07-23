use super::*;
use mwcc_syntax_trees::{ConditionalOrigin, Parameter};

fn name(value: &str) -> Expression {
    Expression::Variable(value.to_string())
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

fn dereference(pointer: &str) -> Expression {
    Expression::Dereference {
        pointer: Box::new(name(pointer)),
    }
}

fn cast(expression: Expression) -> Expression {
    Expression::Cast {
        target_type: Type::Pointer(Pointee::UnsignedChar),
        operand: Box::new(expression),
    }
}

fn add_one(value: &str) -> Expression {
    binary(BinaryOperator::Add, name(value), integer(1))
}

fn assign(target: &str, value: Expression) -> Expression {
    Expression::Assign {
        target: Box::new(name(target)),
        value: Box::new(value),
    }
}

fn increment(value: &str) -> Statement {
    Statement::Assign {
        name: value.to_string(),
        value: add_one(value),
    }
}

fn map_access(map: &str, cursor: &str, masked: bool) -> Expression {
    let bucket = binary(BinaryOperator::ShiftRight, dereference(cursor), integer(3));
    let bucket = if masked {
        binary(BinaryOperator::BitAnd, bucket, integer(31))
    } else {
        bucket
    };
    Expression::Index {
        base: Box::new(name(map)),
        index: Box::new(bucket),
    }
}

fn bit_mask(cursor: &str) -> Expression {
    binary(
        BinaryOperator::ShiftLeft,
        integer(1),
        binary(BinaryOperator::BitAnd, dereference(cursor), integer(7)),
    )
}

fn map_test(map: &str, cursor: &str) -> Expression {
    binary(
        BinaryOperator::BitAnd,
        map_access(map, cursor, true),
        bit_mask(cursor),
    )
}

fn local(name: &str, declared_type: Type, array_length: Option<u16>) -> LocalDeclaration {
    LocalDeclaration {
        declared_type,
        name: name.to_string(),
        initializer: None,
        is_volatile: false,
        array_length,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    }
}

fn tokenizer() -> Function {
    let string = "input";
    let control = "separators";
    let next_token = "continuation";
    let cursor = "cursor";
    let control_cursor = "separator_cursor";
    let map = "classes";
    let index = "slot";

    let zero_map = Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(assign(index, integer(0))),
        condition: Some(binary(BinaryOperator::Less, name(index), integer(32))),
        step: Some(assign(index, add_one(index))),
        body: vec![Statement::Store {
            target: Expression::Index {
                base: Box::new(name(map)),
                index: Box::new(name(index)),
            },
            value: integer(0),
        }],
    };
    let old_map_byte = map_access(map, control_cursor, false);
    let build_map = Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition: Some(binary(
            BinaryOperator::NotEqual,
            Expression::Dereference {
                pointer: Box::new(Expression::PostStep {
                    target: Box::new(name(control_cursor)),
                    operator: BinaryOperator::Add,
                }),
            },
            integer(0),
        )),
        step: None,
        body: vec![Statement::Store {
            target: map_access(map, control_cursor, false),
            value: Expression::IndexedUpdateValue {
                value: Box::new(binary(
                    BinaryOperator::BitOr,
                    old_map_byte,
                    bit_mask(control_cursor),
                )),
            },
        }],
    };
    let choose_cursor = Statement::Assign {
        name: cursor.to_string(),
        value: Expression::Conditional {
            condition: Box::new(name(string)),
            when_true: Box::new(cast(name(string))),
            when_false: Box::new(cast(dereference(next_token))),
            origin: ConditionalOrigin::Ternary,
        },
    };
    let skip = Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(binary(
            BinaryOperator::LogicalAnd,
            map_test(map, cursor),
            binary(BinaryOperator::NotEqual, dereference(cursor), integer(0)),
        )),
        step: None,
        body: vec![increment(cursor)],
    };
    let scan = Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(binary(
            BinaryOperator::NotEqual,
            dereference(cursor),
            integer(0),
        )),
        step: None,
        body: vec![
            Statement::If {
                condition: map_test(map, cursor),
                then_body: vec![
                    Statement::Store {
                        target: dereference(cursor),
                        value: integer(0),
                    },
                    increment(cursor),
                    Statement::Break,
                ],
                else_body: Vec::new(),
            },
            increment(cursor),
        ],
    };

    Function {
        return_type: Type::Pointer(Pointee::UnsignedChar),
        name: "renamed_tokenizer".to_string(),
        is_static: false,
        is_weak: false,
        parameters: vec![
            Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: string.to_string(),
            },
            Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: control.to_string(),
            },
            Parameter {
                parameter_type: Type::Pointer(Pointee::Pointer),
                name: next_token.to_string(),
            },
        ],
        locals: vec![
            local(cursor, Type::Pointer(Pointee::UnsignedChar), None),
            local(control_cursor, Type::Pointer(Pointee::UnsignedChar), None),
            local(map, Type::UnsignedChar, Some(32)),
            local("unused", Type::Int, None),
            local(index, Type::Int, None),
        ],
        statements: vec![
            zero_map,
            Statement::Assign {
                name: control_cursor.to_string(),
                value: cast(name(control)),
            },
            build_map,
            choose_cursor,
            skip,
            Statement::Assign {
                name: string.to_string(),
                value: cast(name(cursor)),
            },
            scan,
            Statement::Store {
                target: dereference(next_token),
                value: cast(name(cursor)),
            },
            Statement::If {
                condition: binary(BinaryOperator::Equal, name(string), cast(name(cursor))),
                then_body: vec![Statement::Assign {
                    name: string.to_string(),
                    value: integer(0),
                }],
                else_body: Vec::new(),
            },
        ],
        guards: Vec::new(),
        return_expression: Some(name(string)),
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
fn recognizes_semantics_independently_of_names() {
    assert!(recognize::recognize(&tokenizer()).is_some());
}

#[test]
fn rejects_a_partial_byte_class_map() {
    let mut function = tokenizer();
    function.locals[2].array_length = Some(16);
    assert!(recognize::recognize(&function).is_none());
}

#[test]
fn rejects_a_token_scan_without_continuation_writeback() {
    let mut function = tokenizer();
    function.statements.remove(7);
    assert!(recognize::recognize(&function).is_none());
}
