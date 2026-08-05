//! Semantic recognition for counted scalar stack-pack loops.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct PackLoop<'a> {
    pub(super) wrapper: &'a str,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn assignment(expression: &Expression, target: &str, value: i64) -> bool {
    matches!(expression, Expression::Assign { target: found, value: assigned }
        if var(found, target) && constant_value(assigned) == Some(value))
}

pub(super) fn classify_pack_loop(function: &Function) -> Option<PackLoop<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || data.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || count.parameter_type != Type::Int
    {
        return None;
    }
    let [error, index] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || index.declared_type != Type::Int
        || error.initializer.is_some()
        || index.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Comma { left, right }),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !assignment(left, &index.name, 0) || !assignment(right, &error.name, 0) {
        return None;
    }
    if !matches!(condition, Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } if matches!(left.as_ref(), Expression::Binary {
            operator: BinaryOperator::Equal, left, right
        } if var(left, &error.name) && constant_value(right) == Some(0))
        && matches!(right.as_ref(), Expression::Binary {
            operator: BinaryOperator::Less, left, right
        } if var(left, &index.name) && var(right, &count.name)))
    {
        return None;
    }
    if !matches!(step, Expression::Assign { target, value }
        if var(target, &index.name)
            && matches!(value.as_ref(), Expression::Binary {
                operator: BinaryOperator::Add, left, right
            } if var(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_error,
        value: Expression::Call { name: wrapper, arguments },
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_error != &error.name
        || !matches!(arguments.as_slice(), [call_buffer, Expression::Index { base, index: subscript }]
            if var(call_buffer, &buffer.name)
                && var(base, &data.name)
                && var(subscript, &index.name))
    {
        return None;
    }
    Some(PackLoop { wrapper })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn recognizes_a_counted_word_pack_loop() {
        let function = Function {
            return_type: Type::Int,
            name: "append_words".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter { parameter_type: Type::StructPointer { element_size: 32 }, name: "buffer".into() },
                Parameter { parameter_type: Type::Pointer(Pointee::UnsignedInt), name: "data".into() },
                Parameter { parameter_type: Type::Int, name: "count".into() },
            ],
            locals: vec![local("error"), local("index")],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(Expression::Comma {
                    left: Box::new(Expression::Assign { target: Box::new(Expression::Variable("index".into())), value: Box::new(Expression::IntegerLiteral(0)) }),
                    right: Box::new(Expression::Assign { target: Box::new(Expression::Variable("error".into())), value: Box::new(Expression::IntegerLiteral(0)) }),
                }),
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    left: Box::new(Expression::Binary { operator: BinaryOperator::Equal, left: Box::new(Expression::Variable("error".into())), right: Box::new(Expression::IntegerLiteral(0)) }),
                    right: Box::new(Expression::Binary { operator: BinaryOperator::Less, left: Box::new(Expression::Variable("index".into())), right: Box::new(Expression::Variable("count".into())) }),
                }),
                step: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("index".into())),
                    value: Box::new(Expression::Binary { operator: BinaryOperator::Add, left: Box::new(Expression::Variable("index".into())), right: Box::new(Expression::IntegerLiteral(1)) }),
                }),
                body: vec![Statement::Assign {
                    name: "error".into(),
                    value: Expression::Call {
                        name: "append_word".into(),
                        arguments: vec![Expression::Variable("buffer".into()), Expression::Index { base: Box::new(Expression::Variable("data".into())), index: Box::new(Expression::Variable("index".into())) }],
                    },
                }],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("error".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert_eq!(classify_pack_loop(&function).unwrap().wrapper, "append_word");
    }
}
