//! Semantic recognition for scalar-read wrappers and their bounded read body.

#[allow(unused_imports)]
use super::super::*;

pub(super) enum StackUnpack<'a> {
    Direct(DirectStackUnpack<'a>),
    Endian(EndianStackUnpack<'a>),
}

impl StackUnpack<'_> {
    pub(super) fn callee(&self) -> &str {
        match self {
            Self::Direct(plan) => plan.callee,
            Self::Endian(plan) => plan.callee,
        }
    }
}

pub(super) struct DirectStackUnpack<'a> {
    pub(super) callee: &'a str,
    pub(super) width: u8,
}

pub(super) struct EndianStackUnpack<'a> {
    pub(super) flag: &'a str,
    pub(super) callee: &'a str,
    pub(super) width: u8,
}

#[derive(Clone)]
pub(super) struct InlineRead {
    pub(super) copy_callee: String,
    pub(super) length_offset: i16,
    pub(super) position_offset: i16,
    pub(super) data_offset: i16,
    pub(super) error_code: i16,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn cast_of(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Cast { operand, .. } if var(operand, name))
        || var(expression, name)
}

fn member_offset(expression: &Expression, base_name: &str, member_type: Type) -> Option<u32> {
    match expression {
        Expression::Member {
            base,
            offset,
            member_type: found_type,
            index_stride: None,
        } if var(base, base_name) && *found_type == member_type => Some(*offset),
        _ => None,
    }
}

fn byte_member_address(expression: &Expression, base_name: &str) -> Option<u32> {
    match expression {
        Expression::MemberAddress {
            base,
            offset,
            element: Pointee::UnsignedChar,
            index_stride: None,
        } if var(base, base_name) => Some(*offset),
        _ => None,
    }
}

pub(super) fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<StackUnpack<'a>> {
    classify_direct(function)
        .map(StackUnpack::Direct)
        .or_else(|| classify_endian(function, globals).map(StackUnpack::Endian))
}

fn classify_direct(function: &Function) -> Option<DirectStackUnpack<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !function.locals.is_empty()
        || !function.statements.is_empty()
    {
        return None;
    }
    let [buffer, data] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || data.parameter_type != Type::Pointer(Pointee::UnsignedChar)
    {
        return None;
    }
    let Expression::Call {
        name: callee,
        arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let [call_buffer, call_data, call_width] = arguments.as_slice() else {
        return None;
    };
    let width = u8::try_from(constant_value(call_width)?).ok()?;
    if !var(call_buffer, &buffer.name)
        || !cast_of(call_data, &data.name)
        || width != 1
    {
        return None;
    }
    Some(DirectStackUnpack { callee, width })
}

fn classify_endian<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<EndianStackUnpack<'a>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let width = match data.parameter_type {
        Type::Pointer(Pointee::UnsignedShort) => 2,
        Type::Pointer(Pointee::UnsignedInt) => 4,
        Type::Pointer(Pointee::UnsignedLongLong) => 8,
        _ => return None,
    };
    let [error, selected, bytes, swapped] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || error.initializer.is_some()
        || selected.declared_type != Type::Pointer(Pointee::UnsignedChar)
        || bytes.declared_type != Type::Pointer(Pointee::UnsignedChar)
        || swapped.declared_type != Type::UnsignedChar
        || swapped.array_length.is_none()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }
    let [select, read, reverse] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: Expression::Variable(flag),
        then_body,
        else_body,
    } = select
    else {
        return None;
    };
    if !globals.contains_key(flag)
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value }]
            if name == &selected.name && cast_of(value, &data.name))
        || !matches!(else_body.as_slice(), [Statement::Assign { name, value }]
            if name == &selected.name && var(value, &swapped.name))
    {
        return None;
    }
    let Statement::Assign {
        name: error_name,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    } = read
    else {
        return None;
    };
    if error_name != &error.name
        || !matches!(arguments.as_slice(), [call_buffer, call_data, call_width]
            if var(call_buffer, &buffer.name)
                && cast_of(call_data, &selected.name)
                && constant_value(call_width) == Some(i64::from(width)))
    {
        return None;
    }
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = reverse
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd, left, right
        } if matches!(left.as_ref(), Expression::Unary {
                operator: UnaryOperator::LogicalNot, operand
            } if var(operand, flag))
            && matches!(right.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal, left, right
            } if var(left, &error.name) && constant_value(right) == Some(0)))
    {
        return None;
    }
    let [Statement::Assign {
        name: bytes_name,
        value: bytes_value,
    }, stores @ ..] = then_body.as_slice()
    else {
        return None;
    };
    if bytes_name != &bytes.name
        || !cast_of(bytes_value, &data.name)
        || stores.len() != usize::from(width)
    {
        return None;
    }
    for (destination, statement) in stores.iter().enumerate() {
        if !matches!(statement, Statement::Store {
            target: Expression::Index { base, index },
            value: Expression::Index { base: source, index: source_index },
        } if var(base, &bytes.name)
            && constant_value(index) == Some(destination as i64)
            && var(source, &selected.name)
            && constant_value(source_index)
                == Some(i64::from(width) - 1 - destination as i64))
        {
            return None;
        }
    }
    Some(EndianStackUnpack {
        flag,
        callee,
        width,
    })
}

/// Validate the complete bounded-read transaction before a wrapper composes it.
/// Offsets and constants are extracted from the definition so emission remains
/// independent of source names and rejects unrelated same-signature callees.
pub(super) fn classify_inline_read(function: &Function) -> Option<InlineRead> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, length] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !matches!(data.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || length.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [error, remaining] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || error.initializer.as_ref().and_then(constant_value) != Some(0)
        || remaining.declared_type != Type::UnsignedInt
        || remaining.initializer.is_some()
    {
        return None;
    }
    let [zero_guard, remaining_assignment, clamp, copy, position_update] =
        function.statements.as_slice()
    else {
        return None;
    };

    let Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Equal,
            left: zero_length,
            right: zero,
        },
        then_body: zero_body,
        else_body: zero_else,
    } = zero_guard
    else {
        return None;
    };
    if !var(zero_length, &length.name)
        || constant_value(zero) != Some(0)
        || !zero_else.is_empty()
        || !matches!(zero_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(0))
    {
        return None;
    }

    let Statement::Assign {
        name: remaining_name,
        value: Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: total_length,
            right: current_position,
        },
    } = remaining_assignment
    else {
        return None;
    };
    let length_offset = i16::try_from(member_offset(
        total_length,
        &buffer.name,
        Type::UnsignedInt,
    )?)
    .ok()?;
    let position_offset = i16::try_from(member_offset(
        current_position,
        &buffer.name,
        Type::UnsignedInt,
    )?)
    .ok()?;
    if remaining_name != &remaining.name {
        return None;
    }

    let Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Greater,
            left: requested,
            right: available,
        },
        then_body: clamp_body,
        else_body: clamp_else,
    } = clamp
    else {
        return None;
    };
    let [Statement::Assign {
        name: error_name,
        value: error_value,
    }, Statement::Assign {
        name: length_name,
        value: clamped_length,
    }] = clamp_body.as_slice()
    else {
        return None;
    };
    let error_code = i16::try_from(constant_value(error_value)?).ok()?;
    if !var(requested, &length.name)
        || !var(available, &remaining.name)
        || !clamp_else.is_empty()
        || error_name != &error.name
        || length_name != &length.name
        || !var(clamped_length, &remaining.name)
    {
        return None;
    }

    let Statement::Expression(Expression::Call {
        name: copy_callee,
        arguments: copy_arguments,
    }) = copy
    else {
        return None;
    };
    let [copy_destination, copy_source, copy_length] = copy_arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: source_base,
        right: source_position,
    } = copy_source
    else {
        return None;
    };
    let data_offset = i16::try_from(byte_member_address(source_base, &buffer.name)?).ok()?;
    if !var(copy_destination, &data.name)
        || member_offset(source_position, &buffer.name, Type::UnsignedInt)?
            != position_offset as u32
        || !var(copy_length, &length.name)
    {
        return None;
    }

    let Statement::Store {
        target: update_target,
        value: Expression::IndexedUpdateValue { value: updated },
    } = position_update
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_position,
        right: added_length,
    } = updated.as_ref()
    else {
        return None;
    };
    if member_offset(update_target, &buffer.name, Type::UnsignedInt)?
        != position_offset as u32
        || member_offset(old_position, &buffer.name, Type::UnsignedInt)?
            != position_offset as u32
        || !var(added_length, &length.name)
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }

    Some(InlineRead {
        copy_callee: copy_callee.clone(),
        length_offset,
        position_offset,
        data_offset,
        error_code,
    })
}
