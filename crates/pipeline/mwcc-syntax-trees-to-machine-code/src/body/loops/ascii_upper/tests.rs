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

fn function() -> Function {
    let input = "buffer";
    let cursor = "position";
    let range = binary(
        BinaryOperator::LogicalAnd,
        binary(
            BinaryOperator::GreaterEqual,
            dereference(cursor),
            integer(97),
        ),
        binary(BinaryOperator::LessEqual, dereference(cursor), integer(122)),
    );
    Function {
        return_type: Type::Pointer(Pointee::UnsignedChar),
        name: "renamed_uppercase".to_string(),
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            parameter_type: Type::Pointer(Pointee::UnsignedChar),
            name: input.to_string(),
        }],
        locals: vec![LocalDeclaration {
            declared_type: Type::Pointer(Pointee::UnsignedChar),
            name: cursor.to_string(),
            initializer: Some(name(input)),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }],
        statements: vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(binary(
                BinaryOperator::NotEqual,
                dereference(cursor),
                integer(0),
            )),
            step: None,
            body: vec![
                Statement::Store {
                    target: dereference(cursor),
                    value: Expression::Conditional {
                        condition: Box::new(range),
                        when_true: Box::new(binary(
                            BinaryOperator::Subtract,
                            dereference(cursor),
                            integer(32),
                        )),
                        when_false: Box::new(dereference(cursor)),
                        origin: ConditionalOrigin::Ternary,
                    },
                },
                Statement::Assign {
                    name: cursor.to_string(),
                    value: binary(BinaryOperator::Add, name(cursor), integer(1)),
                },
            ],
        }],
        guards: Vec::new(),
        return_expression: Some(name(input)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}

#[test]
fn recognizes_in_place_ascii_uppercase_independently_of_names() {
    assert!(recognize(&function()).is_some());
}

#[test]
fn rejects_a_different_case_delta() {
    let mut function = function();
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    let Statement::Store { value, .. } = &mut body[0] else {
        unreachable!()
    };
    let Expression::Conditional { when_true, .. } = value else {
        unreachable!()
    };
    let Expression::Binary { right, .. } = when_true.as_mut() else {
        unreachable!()
    };
    *right = Box::new(integer(31));
    assert!(recognize(&function).is_none());
}

#[test]
fn rejects_a_non_in_place_store() {
    let mut function = function();
    let Statement::Loop { body, .. } = &mut function.statements[0] else {
        unreachable!()
    };
    let Statement::Store { target, .. } = &mut body[0] else {
        unreachable!()
    };
    *target = dereference("buffer");
    assert!(recognize(&function).is_none());
}
