use super::*;

pub(super) fn matches(function: &Function) -> bool {
    let [manager] = function.parameters.as_slice() else {
        return false;
    };
    let [index] = function.locals.as_slice() else {
        return false;
    };
    if function.return_type != Type::Void
        || manager.parameter_type != (Type::StructPointer { element_size: 116 })
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || index.is_volatile
        || index.is_static
        || index.array_length.is_some()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function_makes_call(function)
        || function.statements.len() != 30
    {
        return false;
    }
    let statements = &function.statements;
    let base = manager.name.as_str();
    let induction = index.name.as_str();

    [8, 12, 16, 20]
        .into_iter()
        .enumerate()
        .all(|(at, offset)| {
            member_integer_store(&statements[at], base, offset, Type::StructPointer { element_size: 0 }, 0)
        })
        && member_integer_store(&statements[4], base, 4, Type::UnsignedInt, 0)
        && member_integer_store(&statements[5], base, 0, Type::UnsignedInt, 0)
        && member_integer_store(&statements[6], base, 112, Type::Int, 1)
        && member_float_store(&statements[7], base, 24, 1.0)
        && member_float_store(&statements[8], base, 28, 1.0)
        && member_float_store(&statements[9], base, 32, 0.5)
        && member_float_store(&statements[10], base, 36, 0.0)
        && member_float_store(&statements[11], base, 40, 0.0)
        && counted_zero_fill(
            &statements[12],
            base,
            induction,
            8,
            &[(44, Pointee::Short)],
        )
        && indexed_integer_store(&statements[13], base, 44, Pointee::Short, 0, 32767)
        && member_integer_store(&statements[14], base, 76, Type::Short, 0)
        && counted_zero_fill(
            &statements[15],
            base,
            induction,
            4,
            &[(60, Pointee::Short), (90, Pointee::UnsignedChar)],
        )
        && member_integer_store(&statements[16], base, 96, Type::UnsignedChar, 0)
        && indexed_integer_store(&statements[17], base, 60, Pointee::Short, 0, 32767)
        && member_integer_store(&statements[18], base, 97, Type::UnsignedChar, 0)
        && [336, 528, 850, 1042, 0, 0]
            .into_iter()
            .enumerate()
            .all(|(index, value)| {
                indexed_integer_store(
                    &statements[19 + index],
                    base,
                    78,
                    Pointee::UnsignedShort,
                    index as i64,
                    value,
                )
            })
        && member_integer_store(&statements[25], base, 104, Type::UnsignedInt, 131331)
        && member_integer_store(&statements[26], base, 108, Type::UnsignedShort, 600)
        && [26, 1, 1]
            .into_iter()
            .enumerate()
            .all(|(index, value)| {
                indexed_integer_store(
                    &statements[27 + index],
                    base,
                    98,
                    Pointee::UnsignedChar,
                    index as i64,
                    value,
                )
            })
}

fn member_integer_store(
    statement: &Statement,
    base: &str,
    offset: u32,
    member_type: Type,
    expected: i64,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Member {
            base: member_base,
            offset: member_offset,
            member_type: actual_type,
            index_stride: None,
        },
        value,
    } if variable(member_base) == Some(base)
        && *member_offset == offset
        && *actual_type == member_type
        && constant_value(value) == Some(expected))
}

fn member_float_store(
    statement: &Statement,
    base: &str,
    offset: u32,
    expected: f64,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Member {
            base: member_base,
            offset: member_offset,
            member_type: Type::Float,
            index_stride: None,
        },
        value: Expression::FloatLiteral(value),
    } if variable(member_base) == Some(base)
        && *member_offset == offset
        && value.to_bits() == expected.to_bits())
}

fn indexed_integer_store(
    statement: &Statement,
    base: &str,
    offset: u32,
    element: Pointee,
    expected_index: i64,
    expected_value: i64,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Index {
            base: member,
            index,
        },
        value,
    } if matches!(member.as_ref(), Expression::MemberAddress {
            base: member_base,
            offset: member_offset,
            element: actual_element,
            index_stride: None,
        } if variable(member_base) == Some(base)
            && *member_offset == offset
            && *actual_element == element)
        && constant_value(index) == Some(expected_index)
        && constant_value(value) == Some(expected_value))
}

fn counted_zero_fill(
    statement: &Statement,
    base: &str,
    induction: &str,
    count: i64,
    targets: &[(u32, Pointee)],
) -> bool {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return false;
    };
    if !assignment_to_constant(initializer, induction, 0)
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left) == Some(induction) && constant_value(right) == Some(count))
        || !increments_by_one(step, induction)
        || body.len() != targets.len()
    {
        return false;
    }
    body.iter().zip(targets).all(|(statement, &(offset, element))| {
        matches!(statement, Statement::Store {
            target: Expression::Index { base: member, index },
            value,
        } if matches!(member.as_ref(), Expression::MemberAddress {
                base: member_base,
                offset: member_offset,
                element: actual_element,
                index_stride: None,
            } if variable(member_base) == Some(base)
                && *member_offset == offset
                && *actual_element == element)
            && variable(index) == Some(induction)
            && constant_value(value) == Some(0))
    })
}

fn assignment_to_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name)
            && matches!(value.as_ref(), Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if variable(left) == Some(name) && constant_value(right) == Some(1)))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_function() {
        let function = Function {
            return_type: Type::Void,
            name: "not_audio_defaults".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(!matches(&function));
    }
}
