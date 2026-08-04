use super::*;

pub(super) fn matches(function: &Function) -> bool {
    let [channel] = function.parameters.as_slice() else {
        return false;
    };
    let [routing_index, byte_index, pointer_index] = function.locals.as_slice() else {
        return false;
    };
    if function.return_type != Type::Void
        || channel.parameter_type != (Type::StructPointer { element_size: 320 })
        || [routing_index, byte_index, pointer_index]
            .iter()
            .any(|local| {
                local.declared_type != Type::Int
                    || local.initializer.is_some()
                    || local.is_volatile
                    || local.is_static
                    || local.array_length.is_some()
            })
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function_makes_call(function)
        || function.statements.len() != 14
    {
        return false;
    }
    let statements = &function.statements;
    let object = channel.name.as_str();
    let initial = [
        (40, Type::Pointer(Pointee::Int)),
        (44, Type::Pointer(Pointee::Int)),
        (48, Type::Int),
        (52, Type::Int),
        (16, Type::StructPointer { element_size: 40 }),
        (12, Type::UnsignedChar),
        (20, Type::UnsignedInt),
        (24, Type::UnsignedInt),
        (28, Type::UnsignedInt),
    ];
    if !initial.into_iter().enumerate().all(|(at, (offset, ty))| {
        member_integer_store(&statements[at], object, offset, ty, 0)
    }) {
        return false;
    }
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = &statements[9]
    else {
        return false;
    };
    if !manager_is_null(condition, object)
        || !builtin_defaults(then_body, object)
        || !copied_defaults(
            else_body,
            object,
            &routing_index.name,
            &byte_index.name,
        )
        || !pointer_clear_loop(&statements[10], object, &pointer_index.name)
        || !member_integer_store(&statements[11], object, 2, Type::UnsignedChar, 0)
        || !serial_increment(&statements[12], object)
        || !serial_zero_repair(&statements[13], object)
    {
        return false;
    }
    true
}

fn manager_is_null(condition: &Expression, object: &str) -> bool {
    matches!(condition, Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } if member(left, object, 4, Type::StructPointer { element_size: 116 })
        && constant_value(right) == Some(0))
}

fn builtin_defaults(body: &[Statement], object: &str) -> bool {
    body.len() == 11
        && [336, 528, 850, 1042, 0, 0]
            .into_iter()
            .enumerate()
            .all(|(index, value)| {
                struct_array_store(&body[index], object, 264, index as i64, value)
            })
        && member_integer_store(&body[6], object, 288, Type::UnsignedInt, 65793)
        && member_integer_store(&body[7], object, 292, Type::UnsignedShort, 600)
        && [26, 1, 1]
            .into_iter()
            .enumerate()
            .all(|(index, value)| {
                byte_array_store(&body[8 + index], object, 184, index as i64, value)
            })
}

fn copied_defaults(
    body: &[Statement],
    object: &str,
    routing_index: &str,
    byte_index: &str,
) -> bool {
    body.len() == 4
        && manager_array_copy_loop(
            &body[0],
            object,
            routing_index,
            6,
            264,
            78,
            Pointee::UnsignedShort,
        )
        && manager_member_copy(&body[1], object, 288, Type::UnsignedInt, 104)
        && manager_member_copy(&body[2], object, 292, Type::UnsignedShort, 108)
        && manager_array_copy_loop(
            &body[3],
            object,
            byte_index,
            3,
            184,
            98,
            Pointee::UnsignedChar,
        )
}

fn manager_array_copy_loop(
    statement: &Statement,
    object: &str,
    induction: &str,
    count: i64,
    destination_offset: u32,
    source_offset: u32,
    element: Pointee,
) -> bool {
    let Some(body) = counted_loop_body(statement, induction, count) else {
        return false;
    };
    let [Statement::Store { target, value }] = body else {
        return false;
    };
    let destination_matches = if element == Pointee::UnsignedShort {
        matches!(target, Expression::Member {
            base,
            offset: 0,
            member_type: Type::UnsignedShort,
            index_stride: Some(2),
        } if matches!(base.as_ref(), Expression::Index { base, index }
            if matches!(base.as_ref(), Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size: 2, align: 2 },
                index_stride: None,
            } if variable(base) == Some(object) && *offset == destination_offset)
            && variable(index) == Some(induction)))
    } else {
        matches!(target, Expression::Index { base, index }
            if member_address(base, object, destination_offset, element)
                && variable(index) == Some(induction))
    };
    destination_matches
        && matches!(value, Expression::Index { base, index }
            if manager_member_address(base, object, source_offset, element)
                && variable(index) == Some(induction))
}

fn manager_member_copy(
    statement: &Statement,
    object: &str,
    destination_offset: u32,
    ty: Type,
    source_offset: u32,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Member {
            base: destination_base,
            offset: actual_destination,
            member_type: destination_type,
            index_stride: None,
        },
        value: Expression::Member {
            base: source_base,
            offset: actual_source,
            member_type: source_type,
            index_stride: None,
        },
    } if variable(destination_base) == Some(object)
        && *actual_destination == destination_offset
        && *destination_type == ty
        && *actual_source == source_offset
        && *source_type == ty
        && manager_pointer(source_base, object))
}

fn pointer_clear_loop(statement: &Statement, object: &str, induction: &str) -> bool {
    let Some(body) = counted_loop_body(statement, induction, 4) else {
        return false;
    };
    matches!(body, [Statement::Store {
        target: Expression::Index { base, index },
        value,
    }] if member_address(base, object, 56, Pointee::Pointer)
        && variable(index) == Some(induction)
        && constant_value(value) == Some(0))
}

fn serial_increment(statement: &Statement, object: &str) -> bool {
    matches!(statement, Statement::Store {
        target,
        value: Expression::IndexedUpdateValue { value },
    } if member(target, object, 294, Type::UnsignedShort)
        && matches!(value.as_ref(), Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if member(left, object, 294, Type::UnsignedShort)
            && constant_value(right) == Some(1)))
}

fn serial_zero_repair(statement: &Statement, object: &str) -> bool {
    matches!(statement, Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        },
        then_body,
        else_body,
    } if matches!(left.as_ref(), Expression::Cast { operand, .. }
            if member(operand, object, 294, Type::UnsignedShort))
        && constant_value(right) == Some(0)
        && matches!(then_body.as_slice(), [statement]
            if member_integer_store(statement, object, 294, Type::UnsignedShort, 1))
        && else_body.is_empty())
}

fn counted_loop_body<'a>(
    statement: &'a Statement,
    induction: &str,
    count: i64,
) -> Option<&'a [Statement]> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    if assignment_to_constant(initializer, induction, 0)
        && matches!(condition, Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left) == Some(induction) && constant_value(right) == Some(count))
        && increments_by_one(step, induction)
    {
        Some(body)
    } else {
        None
    }
}

fn struct_array_store(
    statement: &Statement,
    object: &str,
    offset: u32,
    expected_index: i64,
    expected_value: i64,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Member {
            base,
            offset: 0,
            member_type: Type::UnsignedShort,
            index_stride: Some(2),
        },
        value,
    } if matches!(base.as_ref(), Expression::Index { base, index }
            if matches!(base.as_ref(), Expression::Member {
                base,
                offset: actual_offset,
                member_type: Type::Struct { size: 2, align: 2 },
                index_stride: None,
            } if variable(base) == Some(object) && *actual_offset == offset)
            && constant_value(index) == Some(expected_index))
        && constant_value(value) == Some(expected_value))
}

fn byte_array_store(
    statement: &Statement,
    object: &str,
    offset: u32,
    expected_index: i64,
    expected_value: i64,
) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Index { base, index },
        value,
    } if member_address(base, object, offset, Pointee::UnsignedChar)
        && constant_value(index) == Some(expected_index)
        && constant_value(value) == Some(expected_value))
}

fn member_integer_store(
    statement: &Statement,
    object: &str,
    offset: u32,
    ty: Type,
    expected: i64,
) -> bool {
    matches!(statement, Statement::Store { target, value }
        if member(target, object, offset, ty) && constant_value(value) == Some(expected))
}

fn member(expression: &Expression, object: &str, offset: u32, ty: Type) -> bool {
    matches!(expression, Expression::Member {
        base,
        offset: actual_offset,
        member_type,
        index_stride: None,
    } if variable(base) == Some(object) && *actual_offset == offset && *member_type == ty)
}

fn member_address(
    expression: &Expression,
    object: &str,
    offset: u32,
    element: Pointee,
) -> bool {
    matches!(expression, Expression::MemberAddress {
        base,
        offset: actual_offset,
        element: actual_element,
        index_stride: None,
    } if variable(base) == Some(object)
        && *actual_offset == offset
        && *actual_element == element)
}

fn manager_member_address(
    expression: &Expression,
    object: &str,
    offset: u32,
    element: Pointee,
) -> bool {
    matches!(expression, Expression::MemberAddress {
        base,
        offset: actual_offset,
        element: actual_element,
        index_stride: None,
    } if manager_pointer(base, object)
        && *actual_offset == offset
        && *actual_element == element)
}

fn manager_pointer(expression: &Expression, object: &str) -> bool {
    member(
        expression,
        object,
        4,
        Type::StructPointer { element_size: 116 },
    )
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
            name: "not_channel_defaults".into(),
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
