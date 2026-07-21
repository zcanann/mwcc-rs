use super::*;
use mwcc_syntax_trees::{LocalDeclaration, Parameter};

fn name(value: &str) -> Expression {
    Expression::Variable(value.to_string())
}

fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::UnsignedInt,
        name: name.to_string(),
        initializer,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    }
}

fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
    Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn function() -> Function {
    let byte = "value";
    let accumulator = "result";
    let pointer = "text";
    let shifted = binary(
        BinaryOperator::ShiftRight,
        Expression::Cast {
            target_type: Type::Int,
            operand: Box::new(name(byte)),
        },
        Expression::IntegerLiteral(1),
    );
    let case_bit = binary(
        BinaryOperator::BitAnd,
        binary(BinaryOperator::BitAnd, name(byte), shifted),
        Expression::IntegerLiteral(0x20),
    );
    let fold = binary(
        BinaryOperator::BitAnd,
        binary(BinaryOperator::Subtract, name(byte), case_bit),
        Expression::IntegerLiteral(0xff),
    );
    let update = binary(
        BinaryOperator::Add,
        fold,
        binary(
            BinaryOperator::Multiply,
            name(accumulator),
            Expression::IntegerLiteral(131),
        ),
    );
    Function {
        return_type: Type::UnsignedInt,
        name: "renamed_hash".to_string(),
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            parameter_type: Type::Pointer(Pointee::UnsignedChar),
            name: pointer.to_string(),
        }],
        locals: vec![
            local(accumulator, Some(Expression::IntegerLiteral(0))),
            local(byte, None),
        ],
        statements: vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(name(byte)),
                    value: Box::new(Expression::Dereference {
                        pointer: Box::new(name(pointer)),
                    }),
                }),
                right: Box::new(binary(
                    BinaryOperator::NotEqual,
                    name(byte),
                    Expression::IntegerLiteral(0),
                )),
            }),
            step: None,
            body: vec![
                Statement::Assign {
                    name: accumulator.to_string(),
                    value: update,
                },
                Statement::Assign {
                    name: pointer.to_string(),
                    value: binary(
                        BinaryOperator::Add,
                        name(pointer),
                        Expression::IntegerLiteral(1),
                    ),
                },
            ],
        }],
        guards: Vec::new(),
        return_expression: Some(name(accumulator)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}

fn bounded_function() -> Function {
    let mut function = function();
    function.parameters.push(Parameter {
        parameter_type: Type::UnsignedInt,
        name: "limit".to_string(),
    });
    function
        .locals
        .insert(1, local("position", Some(Expression::IntegerLiteral(0))));
    let Statement::Loop {
        condition, body, ..
    } = &mut function.statements[0]
    else {
        unreachable!()
    };
    let byte_test = condition.take().unwrap();
    *condition = Some(binary(
        BinaryOperator::LogicalAnd,
        binary(BinaryOperator::Less, name("position"), name("limit")),
        byte_test,
    ));
    let hash_update = body.remove(0);
    let pointer_increment = body.remove(0);
    *body = vec![
        Statement::Assign {
            name: "position".to_string(),
            value: binary(
                BinaryOperator::Add,
                name("position"),
                Expression::IntegerLiteral(1),
            ),
        },
        pointer_increment,
        hash_update,
    ];
    function
}

fn prefix_seeded_function() -> Function {
    let mut function = function();
    function.parameters.insert(
        0,
        Parameter {
            parameter_type: Type::UnsignedInt,
            name: "prefix".to_string(),
        },
    );
    function.locals[0].initializer = None;
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    body.swap(0, 1);
    function
}

#[test]
fn recognizes_semantics_independently_of_names_and_plain_char_signedness() {
    let mut function = function();
    assert!(recognize(&function).is_some());
    function.parameters[0].parameter_type = Type::Pointer(Pointee::Char);
    assert!(recognize(&function).is_some());
}

#[test]
fn recognizes_bounded_and_prefix_seeded_forms() {
    assert!(matches!(
        recognize(&bounded_function()),
        Some(AsciiHashLoop::Bounded { .. })
    ));
    assert!(matches!(
        recognize(&prefix_seeded_function()),
        Some(AsciiHashLoop::PrefixSeeded { .. })
    ));
}

#[test]
fn rejects_a_different_hash_multiplier() {
    let mut function = function();
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    let Statement::Assign { value, .. } = &mut body[0] else {
        unreachable!()
    };
    let Expression::Binary { right, .. } = value else {
        unreachable!()
    };
    let Expression::Binary { right, .. } = right.as_mut() else {
        unreachable!()
    };
    *right = Box::new(Expression::IntegerLiteral(33));
    assert!(recognize(&function).is_none());
}
