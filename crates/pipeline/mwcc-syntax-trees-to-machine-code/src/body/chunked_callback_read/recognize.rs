//! Semantic recognition for an aligned chunked resource reader.

use super::*;

pub(super) struct ChunkedCallbackRead {
    pub(super) callback: String,
    pub(super) fallback: String,
    pub(super) copy: String,
    pub(super) size_offset: i16,
    pub(super) position_offset: i16,
    pub(super) data_offset: i16,
    pub(super) buffer_offset: i16,
    pub(super) chunk_size: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn casted_variable(mut expression: &Expression) -> Option<&str> {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    variable(expression)
}

fn member(expression: &Expression, owner: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (variable(base)? == owner).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn assignment<'a>(statement: &'a Statement, expected: &str) -> Option<&'a Expression> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    (name == expected).then_some(value)
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(expression) = statement else {
        return None;
    };
    direct_call(expression)
}

fn zero(expression: &Expression) -> bool {
    constant_value(expression) == Some(0)
}

fn return_zero(statements: &[Statement]) -> bool {
    matches!(
        statements,
        [Statement::Return(Some(value))] if zero(value)
    )
}

fn member_argument(expression: &Expression, owner: &str, offset: i16) -> bool {
    member(expression, owner).is_some_and(|(candidate, _)| candidate == offset)
}

fn ordered_add_variables(expression: &Expression, left_name: &str, right_name: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if variable(left) == Some(left_name) && variable(right) == Some(right_name)
    )
}

pub(super) fn classify(function: &Function) -> Option<ChunkedCallbackRead> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.locals.len() != 4
        || function.locals.iter().any(|local| {
            local.declared_type != Type::Int
                || local.initializer.is_some()
                || local.array_length.is_some()
                || local.is_static
        })
        || constant_value(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [object, target, remaining] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::StructPointer { .. } | Type::Pointer(_)
    ) || !matches!(
        target.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || remaining.parameter_type != Type::Int
    {
        return None;
    }
    let [offset_assign, size_assign, clamp, empty, read_loop] = function.statements.as_slice()
    else {
        return None;
    };

    let Statement::Assign {
        name: offset_name,
        value: offset_value,
    } = offset_assign
    else {
        return None;
    };
    let (position_offset, position_type) = member(offset_value, &object.name)?;
    let Statement::Assign {
        name: size_name,
        value: size_value,
    } = size_assign
    else {
        return None;
    };
    let (size_offset, size_type) = member(size_value, &object.name)?;
    if !matches!(position_type, Type::Int | Type::UnsignedInt)
        || !matches!(size_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: summed,
                right: limit,
            },
        then_body,
        else_body,
    } = clamp
    else {
        return None;
    };
    let [clamped] = then_body.as_slice() else {
        return None;
    };
    if !else_body.is_empty()
        || !ordered_add_variables(summed, offset_name, &remaining.name)
        || variable(limit) != Some(size_name)
        || !matches!(
            assignment(clamped, &remaining.name),
            Some(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            }) if variable(left) == Some(size_name) && variable(right) == Some(offset_name)
        )
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
        then_body,
        else_body,
    } = empty
    else {
        return None;
    };
    let [Statement::Store {
        target: empty_target,
        value: empty_value,
    }, Statement::Return(Some(empty_result))] = then_body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || variable(left) != Some(&remaining.name)
        || !zero(right)
        || constant_value(empty_value) != Some(255)
        || !zero(empty_result)
        || !matches!(
            empty_target,
            Expression::Dereference { pointer }
                if casted_variable(pointer) == Some(&target.name)
        )
    {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = read_loop
    else {
        return None;
    };
    if !matches!(
        condition,
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if variable(left) == Some(&remaining.name) && zero(right)
    ) {
        return None;
    }
    let [used_assign, cap_used, aligned_assign, extra_assign, rounded_assign, callback_choice, copy_guard, target_advance, remaining_decrement, position_advance] =
        body.as_slice()
    else {
        return None;
    };
    let Statement::Assign {
        name: used_name,
        value: used_value,
    } = used_assign
    else {
        return None;
    };
    if variable(used_value) != Some(&remaining.name) {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: cap_value,
                right: chunk,
            },
        then_body: cap_body,
        else_body: cap_else,
    } = cap_used
    else {
        return None;
    };
    let [capped] = cap_body.as_slice() else {
        return None;
    };
    let chunk_size = i16::try_from(constant_value(chunk)?).ok()?;
    if !cap_else.is_empty()
        || variable(cap_value) != Some(used_name)
        || constant_value(assignment(capped, used_name)?) != Some(i64::from(chunk_size))
    {
        return None;
    }

    let aligned_value = assignment(aligned_assign, offset_name)?;
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: aligned_position,
        right: aligned_mask,
    } = aligned_value
    else {
        return None;
    };
    if member(aligned_position, &object.name)?.0 != position_offset
        || constant_value(aligned_mask)? as u64 != 0xffff_fffc
    {
        return None;
    }
    let Statement::Assign {
        name: extra_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: extra_position,
                right: extra_mask,
            },
    } = extra_assign
    else {
        return None;
    };
    if member(extra_position, &object.name)?.0 != position_offset
        || constant_value(extra_mask) != Some(3)
    {
        return None;
    }
    let rounded = assignment(rounded_assign, size_name)?;
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: rounded_sum,
        right: rounded_mask,
    } = rounded
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: used_and_extra,
        right: bias,
    } = rounded_sum.as_ref()
    else {
        return None;
    };
    if !ordered_add_variables(used_and_extra, used_name, extra_name)
        || constant_value(bias) != Some(31)
        || constant_value(rounded_mask)? as u64 != 0xffff_ffe0
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: callback_value,
                right: callback_zero,
            },
        then_body: callback_body,
        else_body: fallback_body,
    } = callback_choice
    else {
        return None;
    };
    let callback = variable(callback_value)?;
    if !zero(callback_zero) {
        return None;
    }
    let [callback_statement] = callback_body.as_slice() else {
        return None;
    };
    let (called_callback, callback_arguments) = call_statement(callback_statement)?;
    let [callback_data, callback_buffer, callback_size, callback_offset, callback_user] =
        callback_arguments
    else {
        return None;
    };
    let data_offset = member(callback_data, &object.name)?.0;
    let buffer_offset = member(callback_buffer, &object.name)?.0;
    if called_callback != callback
        || variable(callback_size) != Some(size_name)
        || variable(callback_offset) != Some(offset_name)
        || !zero(callback_user)
    {
        return None;
    }
    let [fallback_statement] = fallback_body.as_slice() else {
        return None;
    };
    let (fallback, fallback_arguments) = call_statement(fallback_statement)?;
    let [fallback_data, fallback_buffer, fallback_size, fallback_offset, priority] =
        fallback_arguments
    else {
        return None;
    };
    if !member_argument(fallback_data, &object.name, data_offset)
        || !member_argument(fallback_buffer, &object.name, buffer_offset)
        || variable(fallback_size) != Some(size_name)
        || variable(fallback_offset) != Some(offset_name)
        || constant_value(priority) != Some(2)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: copy_call,
            },
        then_body: copy_failure,
        else_body: copy_else,
    } = copy_guard
    else {
        return None;
    };
    let (copy, copy_arguments) = direct_call(copy_call)?;
    let [copy_target, copy_source, copy_size] = copy_arguments else {
        return None;
    };
    let mut copy_source = copy_source;
    while let Expression::Cast { operand, .. } = copy_source {
        copy_source = operand;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: source_buffer,
        right: source_extra,
    } = copy_source
    else {
        return None;
    };
    let mut source_buffer = source_buffer.as_ref();
    while let Expression::Cast { operand, .. } = source_buffer {
        source_buffer = operand;
    }
    if casted_variable(copy_target) != Some(&target.name)
        || member(source_buffer, &object.name)?.0 != buffer_offset
        || variable(source_extra) != Some(extra_name)
        || variable(copy_size) != Some(used_name)
        || !copy_else.is_empty()
        || !return_zero(copy_failure)
    {
        return None;
    }

    let target_value = assignment(target_advance, &target.name)?;
    let mut target_value = target_value;
    while let Expression::Cast { operand, .. } = target_value {
        target_value = operand;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_target,
        right: target_step,
    } = target_value
    else {
        return None;
    };
    if casted_variable(old_target) != Some(&target.name)
        || variable(target_step) != Some(used_name)
        || !matches!(
            assignment(remaining_decrement, &remaining.name),
            Some(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            }) if variable(left) == Some(&remaining.name) && variable(right) == Some(used_name)
        )
    {
        return None;
    }
    let Statement::Store {
        target: advanced_position,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: old_position,
                right: position_step,
            },
    } = position_advance
    else {
        return None;
    };
    if member(advanced_position, &object.name)?.0 != position_offset
        || member(old_position, &object.name)?.0 != position_offset
        || variable(position_step) != Some(used_name)
    {
        return None;
    }

    Some(ChunkedCallbackRead {
        callback: callback.to_owned(),
        fallback: fallback.to_owned(),
        copy: copy.to_owned(),
        size_offset,
        position_offset,
        data_offset,
        buffer_offset,
        chunk_size,
    })
}
