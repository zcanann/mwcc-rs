//! Semantic recognition for the wrapper and its optional bounded append body.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct EndianStackPack<'a> {
    pub(super) flag: &'a str,
    pub(super) callee: &'a str,
    pub(super) width: u8,
}

#[derive(Clone)]
pub(super) struct InlineAppend {
    pub(super) copy_callee: String,
    pub(super) capacity: i16,
    pub(super) error_code: i16,
    pub(super) count_offset: i16,
    pub(super) mirror_offset: i16,
    pub(super) data_offset: i16,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn address_of_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => address_of_name(operand, name),
        Expression::AddressOf { operand } => var(operand, name),
        _ => false,
    }
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
) -> Option<EndianStackPack<'a>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        buffer.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let width = match data.parameter_type {
        Type::UnsignedShort => 2,
        Type::UnsignedInt => 4,
        Type::UnsignedLongLong => 8,
        _ => return None,
    };
    let [selected, bytes, swapped] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(selected.declared_type, Type::Pointer(Pointee::UnsignedChar))
        || !matches!(bytes.declared_type, Type::Pointer(Pointee::UnsignedChar))
        || swapped.declared_type != Type::UnsignedChar
        || swapped.array_length != Some(width.into())
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(flag),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !globals.contains_key(flag) {
        return None;
    }
    let [Statement::Assign {
        name: selected_then,
        value: native_address,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if selected_then != &selected.name || !address_of_name(native_address, &data.name) {
        return None;
    }
    let [Statement::Assign {
        name: bytes_name,
        value: bytes_address,
    }, Statement::Assign {
        name: selected_else,
        value: swapped_address,
    }, swaps @ ..] = else_body.as_slice()
    else {
        return None;
    };
    if bytes_name != &bytes.name
        || !address_of_name(bytes_address, &data.name)
        || selected_else != &selected.name
        || !var(swapped_address, &swapped.name)
        || swaps.len() != usize::from(width)
    {
        return None;
    }
    for (destination, statement) in swaps.iter().enumerate() {
        let Statement::Store {
            target: Expression::Index { base, index },
            value:
                Expression::Index {
                    base: source,
                    index: source_index,
                },
        } = statement
        else {
            return None;
        };
        if !var(base, &selected.name)
            || constant_value(index) != Some(destination as i64)
            || !var(source, &bytes.name)
            || constant_value(source_index) != Some(i64::from(width) - 1 - destination as i64)
        {
            return None;
        }
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
    if !var(call_buffer, &buffer.name)
        || !matches!(call_data, Expression::Cast { operand, .. } if var(operand, &selected.name))
        || constant_value(call_width) != Some(i64::from(width))
    {
        return None;
    }
    Some(EndianStackPack {
        flag,
        callee,
        width,
    })
}

/// Validate the complete bounded append transaction before a wrapper composes
/// it. The extracted offsets/constants keep emission independent of source
/// names while rejecting an unrelated same-signature callee.
pub(super) fn classify_inline_append(function: &Function) -> Option<InlineAppend> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, length] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::StructPointer { .. } | Type::Pointer(_))
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
    let [zero_guard, remaining_assignment, clamp, copy, count_update, mirror_update] =
        function.statements.as_slice()
    else {
        return None;
    };

    let Statement::If {
        condition:
            Expression::Binary {
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
        || !matches!(zero_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(0))
    {
        return None;
    }

    let Statement::Assign {
        name: remaining_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: capacity,
                right: current_count,
            },
    } = remaining_assignment
    else {
        return None;
    };
    let capacity = i16::try_from(constant_value(capacity)?).ok()?;
    let count_offset = i16::try_from(member_offset(
        current_count,
        &buffer.name,
        Type::UnsignedInt,
    )?)
    .ok()?;
    if remaining_name != &remaining.name {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: available,
                right: requested,
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
    if !var(available, &remaining.name)
        || !var(requested, &length.name)
        || !clamp_else.is_empty()
        || error_name != &error.name
        || length_name != &length.name
        || !var(clamped_length, &remaining.name)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: copy_length,
                right: one,
            },
        then_body: byte_body,
        else_body: block_body,
    } = copy
    else {
        return None;
    };
    if !var(copy_length, &length.name) || constant_value(one) != Some(1) {
        return None;
    }
    let [Statement::Store {
        target:
            Expression::Index {
                base: byte_array,
                index: byte_index,
            },
        value:
            Expression::Index {
                base: source_bytes,
                index: source_index,
            },
    }] = byte_body.as_slice()
    else {
        return None;
    };
    let data_offset = i16::try_from(byte_member_address(byte_array, &buffer.name)?).ok()?;
    if member_offset(byte_index, &buffer.name, Type::UnsignedInt)? != count_offset as u32
        || !matches!(source_bytes.as_ref(), Expression::Cast { operand, .. } if var(operand, &data.name))
        || constant_value(source_index) != Some(0)
    {
        return None;
    }

    let [Statement::Expression(Expression::Call {
        name: copy_callee,
        arguments: copy_arguments,
    })] = block_body.as_slice()
    else {
        return None;
    };
    let [copy_destination, copy_source, copy_length] = copy_arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: destination_base,
        right: destination_index,
    } = copy_destination
    else {
        return None;
    };
    if byte_member_address(destination_base, &buffer.name)? != data_offset as u32
        || member_offset(destination_index, &buffer.name, Type::UnsignedInt)?
            != count_offset as u32
        || !var(copy_source, &data.name)
        || !var(copy_length, &length.name)
    {
        return None;
    }

    let Statement::Store {
        target: count_target,
        value:
            Expression::IndexedUpdateValue {
                value: updated_count,
            },
    } = count_update
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_count,
        right: added_length,
    } = updated_count.as_ref()
    else {
        return None;
    };
    if member_offset(count_target, &buffer.name, Type::UnsignedInt)? != count_offset as u32
        || member_offset(old_count, &buffer.name, Type::UnsignedInt)? != count_offset as u32
        || !var(added_length, &length.name)
    {
        return None;
    }

    let Statement::Store {
        target: mirror_target,
        value: final_count,
    } = mirror_update
    else {
        return None;
    };
    let mirror_offset = i16::try_from(member_offset(
        mirror_target,
        &buffer.name,
        Type::UnsignedInt,
    )?)
    .ok()?;
    if member_offset(final_count, &buffer.name, Type::UnsignedInt)? != count_offset as u32
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }

    Some(InlineAppend {
        copy_callee: copy_callee.clone(),
        capacity,
        error_code,
        count_offset,
        mirror_offset,
        data_offset,
    })
}
