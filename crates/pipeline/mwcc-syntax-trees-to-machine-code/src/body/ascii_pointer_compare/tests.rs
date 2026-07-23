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

fn assign(name: &str, value: Expression) -> Statement {
    Statement::Assign {
        name: name.to_string(),
        value,
    }
}

fn flag_set_if(flag: &str, condition: Expression) -> Statement {
    Statement::If {
        condition,
        then_body: vec![assign(flag, integer(1))],
        else_body: Vec::new(),
    }
}

fn range(pointer: &str) -> Expression {
    binary(
        BinaryOperator::LogicalAnd,
        binary(
            BinaryOperator::GreaterEqual,
            dereference(pointer),
            integer(97),
        ),
        binary(
            BinaryOperator::LessEqual,
            dereference(pointer),
            integer(122),
        ),
    )
}

fn pointer_adjust_if(flag: &str, pointer: &str) -> Statement {
    Statement::If {
        condition: binary(BinaryOperator::NotEqual, name(flag), integer(0)),
        then_body: vec![assign(
            pointer,
            binary(BinaryOperator::Subtract, name(pointer), integer(32)),
        )],
        else_body: Vec::new(),
    }
}

fn flag_sequence(flag: &str, pointer: &str) -> Vec<Statement> {
    vec![
        assign(flag, integer(0)),
        flag_set_if(flag, range(pointer)),
        pointer_adjust_if(flag, pointer),
    ]
}

fn flag_local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::Char,
        name: name.to_string(),
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

fn comparison() -> Function {
    let first = "left_cursor";
    let second = "right_cursor";
    let first_flag = "left_flag";
    let second_flag = "right_flag";

    let mut loop_body = vec![
        assign(first_flag, integer(0)),
        flag_set_if(
            first_flag,
            binary(
                BinaryOperator::LogicalAnd,
                binary(BinaryOperator::NotEqual, dereference(first), integer(122)),
                binary(BinaryOperator::Equal, dereference(first), integer(97)),
            ),
        ),
        pointer_adjust_if(first_flag, first),
    ];
    loop_body.extend(flag_sequence(second_flag, second));
    loop_body.push(Statement::If {
        condition: binary(
            BinaryOperator::LogicalAnd,
            binary(BinaryOperator::Equal, dereference(first), integer(0)),
            binary(BinaryOperator::Equal, dereference(second), integer(0)),
        ),
        then_body: vec![
            assign(first, binary(BinaryOperator::Add, name(first), integer(1))),
            assign(
                second,
                binary(BinaryOperator::Add, name(second), integer(1)),
            ),
        ],
        else_body: vec![Statement::Break],
    });

    let mut mismatch_body = flag_sequence(first_flag, first);
    mismatch_body.extend(flag_sequence(second_flag, second));
    mismatch_body.push(Statement::Return(Some(Expression::Conditional {
        condition: Box::new(binary(
            BinaryOperator::Less,
            Expression::Cast {
                target_type: Type::Int,
                operand: Box::new(dereference(first)),
            },
            Expression::Cast {
                target_type: Type::Int,
                operand: Box::new(dereference(second)),
            },
        )),
        when_true: Box::new(integer(-1)),
        when_false: Box::new(integer(1)),
        origin: ConditionalOrigin::IfReturns,
    })));

    Function {
        return_type: Type::Int,
        name: "renamed_pointer_compare".to_string(),
        is_static: false,
        is_weak: false,
        parameters: vec![
            Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: first.to_string(),
            },
            Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: second.to_string(),
            },
        ],
        locals: vec![flag_local(first_flag), flag_local(second_flag)],
        statements: vec![
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(integer(1)),
                step: None,
                body: loop_body,
            },
            Statement::If {
                condition: binary(
                    BinaryOperator::NotEqual,
                    dereference(first),
                    dereference(second),
                ),
                then_body: mismatch_body,
                else_body: Vec::new(),
            },
        ],
        guards: Vec::new(),
        return_expression: Some(integer(0)),
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
fn recognizes_pointer_adjusting_comparison_independently_of_names() {
    assert!(recognize::recognize(&comparison()).is_some());
}

#[test]
fn rejects_a_conventional_first_ascii_range() {
    let mut function = comparison();
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    let Statement::If { condition, .. } = &mut body[1] else {
        unreachable!()
    };
    *condition = range("left_cursor");
    assert!(recognize::recognize(&function).is_none());
}

#[test]
fn rejects_byte_adjustment_instead_of_pointer_adjustment() {
    let mut function = comparison();
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    let Statement::If { then_body, .. } = &mut body[2] else {
        unreachable!()
    };
    let Statement::Assign { value, .. } = &mut then_body[0] else {
        unreachable!()
    };
    *value = binary(
        BinaryOperator::Subtract,
        dereference("left_cursor"),
        integer(32),
    );
    assert!(recognize::recognize(&function).is_none());
}
