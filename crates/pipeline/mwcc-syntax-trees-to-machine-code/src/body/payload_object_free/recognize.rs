//! Semantic recognition for an event-aware payload release transaction.

use super::*;

pub(super) struct PayloadObjectFree {
    pub(super) free_callee: String,
    pub(super) header_size: i16,
    pub(super) list_offset: i16,
    pub(super) type_offset: i16,
    pub(super) callback_offset: i16,
    pub(super) event: i16,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn casted_var(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => casted_var(operand, expected),
        _ => var(expression, expected),
    }
}

fn dereferenced_var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer } if var(pointer, expected))
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn member(expression: &Expression, base_name: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    var(base, base_name).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn adjusted_object(expression: &Expression, output: &str, subtract: bool) -> Option<i16> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if (*operator == BinaryOperator::Subtract) != subtract {
        return None;
    }
    let header_size = i16::try_from(constant_value(right)?).ok()?;
    (header_size > 0
        && matches!(
            left.as_ref(),
            Expression::Cast { operand, .. } if dereferenced_var(operand, output)
        ))
    .then_some(header_size)
}

pub(super) fn classify(function: &Function) -> Option<PayloadObjectFree> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [output] = function.parameters.as_slice() else {
        return None;
    };
    let [payload] = function.locals.as_slice() else {
        return None;
    };
    if output.parameter_type != Type::Pointer(Pointee::Pointer)
        || !matches!(payload.declared_type, Type::StructPointer { .. })
        || payload.initializer.is_some()
        || payload.is_static
        || payload.is_volatile
        || payload.array_length.is_some()
    {
        return None;
    }

    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: output_present,
                right: object_present,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(
        output_present.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if var(left, &output.name) && is_constant(right, 0)
    ) || !matches!(
        object_present.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if dereferenced_var(left, &output.name) && is_constant(right, 0)
    ) || !else_body.is_empty()
    {
        return None;
    }

    let [Statement::Assign {
        name: payload_name,
        value: loaded_payload,
    }, Statement::Expression(Expression::CallThrough {
        target: callback,
        arguments: callback_arguments,
    }), Statement::Store {
        target: adjusted_target,
        value: adjusted_value,
    }, Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: free_call,
                right: free_failure,
            },
        then_body: failure,
        else_body: free_else,
    }, Statement::Store {
        target: clear_target,
        value: clear_value,
    }, Statement::Return(Some(success))] = then_body.as_slice()
    else {
        return None;
    };

    let Expression::Dereference {
        pointer: payload_pointer,
    } = loaded_payload
    else {
        return None;
    };
    let Expression::Cast {
        operand: payload_address,
        ..
    } = payload_pointer.as_ref()
    else {
        return None;
    };
    let header_size = adjusted_object(payload_address, &output.name, true)?;
    if payload_name != &payload.name
        || !dereferenced_var(adjusted_target, &output.name)
        || adjusted_object(adjusted_value, &output.name, true) != Some(header_size)
        || !is_constant(success, 1)
    {
        return None;
    }

    let Expression::Member {
        base: callback_type,
        offset: callback_offset,
        member_type: callback_member_type,
        index_stride: None,
    } = callback.as_ref()
    else {
        return None;
    };
    let (type_offset, type_member_type) = member(callback_type, &payload.name)?;
    let [callback_object, callback_event, callback_argument] = callback_arguments.as_slice() else {
        return None;
    };
    if !matches!(
        callback_member_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(type_member_type, Type::StructPointer { .. })
        || !dereferenced_var(callback_object, &output.name)
        || !is_constant(callback_argument, 0)
    {
        return None;
    }
    let event = i16::try_from(constant_value(callback_event)?).ok()?;

    let Expression::Call {
        name: free_callee,
        arguments: free_arguments,
    } = free_call.as_ref()
    else {
        return None;
    };
    let [list_argument, output_argument] = free_arguments.as_slice() else {
        return None;
    };
    let (list_offset, list_type) = member(list_argument, &payload.name)?;
    if !matches!(list_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !var(output_argument, &output.name)
        || !is_constant(free_failure, 0)
        || !free_else.is_empty()
        || !matches!(
            failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || !dereferenced_var(clear_target, &output.name)
        || !is_constant(clear_value, 0)
    {
        return None;
    }

    Some(PayloadObjectFree {
        free_callee: free_callee.clone(),
        header_size,
        list_offset,
        type_offset,
        callback_offset: i16::try_from(*callback_offset).ok()?,
        event,
    })
}
