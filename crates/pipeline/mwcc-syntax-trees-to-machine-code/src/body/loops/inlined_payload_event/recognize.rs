//! Semantic recognition for the dispatcher and its retained predicate.

use super::*;

pub(super) struct PayloadPredicate {
    pub(super) registry: String,
    pub(super) test_callee: String,
    pub(super) header_size: i16,
    pub(super) type_offset: i16,
}

pub(super) struct InlinedPayloadEvent {
    pub(super) helper: String,
    pub(super) registry: String,
    pub(super) test_callee: String,
    pub(super) header_size: i16,
    pub(super) type_offset: i16,
    pub(super) callback_offset: i16,
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

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn payload_load(expression: &Expression, object: &str) -> Option<i16> {
    let Expression::Dereference { pointer } = expression else {
        return None;
    };
    let Expression::Cast { operand, .. } = pointer.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = operand.as_ref()
    else {
        return None;
    };
    let header_size = i16::try_from(constant_value(right)?).ok()?;
    (header_size > 0 && casted_var(left, object)).then_some(header_size)
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

pub(super) fn classify_predicate(function: &Function) -> Option<PayloadPredicate> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [object, requested_type] = function.parameters.as_slice() else {
        return None;
    };
    let [payload] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::Pointer(_))
        || !matches!(requested_type.parameter_type, Type::StructPointer { .. })
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
                operator: BinaryOperator::NotEqual,
                left: tested_object,
                right: null_object,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !var(tested_object, &object.name) || !is_constant(null_object, 0) || !else_body.is_empty() {
        return None;
    }
    let [Statement::Assign {
        name: payload_name,
        value: loaded_payload,
    }, Statement::If {
        condition:
            Expression::Call {
                name: test_callee,
                arguments: test_arguments,
            },
        then_body: test_success,
        else_body: test_else,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [Expression::Variable(registry), tested_payload] = test_arguments.as_slice() else {
        return None;
    };
    if payload_name != &payload.name || !var(tested_payload, &payload.name) || !test_else.is_empty()
    {
        return None;
    }
    let header_size = payload_load(loaded_payload, &object.name)?;
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: payload_type,
                right: compared_type,
            },
        then_body: type_success,
        else_body: type_else,
    }] = test_success.as_slice()
    else {
        return None;
    };
    let (type_offset, type_member_type) = member(payload_type, &payload.name)?;
    if !matches!(type_member_type, Type::StructPointer { .. })
        || !var(compared_type, &requested_type.name)
        || !type_else.is_empty()
        || !matches!(
            type_success.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 1)
        )
    {
        return None;
    }

    Some(PayloadPredicate {
        registry: registry.clone(),
        test_callee: test_callee.clone(),
        header_size,
        type_offset,
    })
}

pub(super) fn classify(function: &Function) -> Option<InlinedPayloadEvent> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [object, event, argument] = function.parameters.as_slice() else {
        return None;
    };
    let [payload] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::Pointer(_))
        || event.parameter_type != Type::Int
        || !matches!(argument.parameter_type, Type::Pointer(_))
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
                operator: BinaryOperator::NotEqual,
                left: tested_object,
                right: null_object,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !var(tested_object, &object.name) || !is_constant(null_object, 0) || !else_body.is_empty() {
        return None;
    }
    let [Statement::Assign {
        name: payload_name,
        value: loaded_payload,
    }, Statement::If {
        condition:
            Expression::Call {
                name: test_callee,
                arguments: test_arguments,
            },
        then_body: test_success,
        else_body: test_else,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [Expression::Variable(registry), tested_payload] = test_arguments.as_slice() else {
        return None;
    };
    if payload_name != &payload.name || !var(tested_payload, &payload.name) || !test_else.is_empty()
    {
        return None;
    }
    let header_size = payload_load(loaded_payload, &object.name)?;

    let [Statement::If {
        condition:
            Expression::Call {
                name: helper,
                arguments: helper_arguments,
            },
        then_body: helper_success,
        else_body: helper_else,
    }] = test_success.as_slice()
    else {
        return None;
    };
    let [tested_object, requested_type] = helper_arguments.as_slice() else {
        return None;
    };
    let (type_offset, type_member_type) = member(requested_type, &payload.name)?;
    if !var(tested_object, &object.name)
        || !matches!(type_member_type, Type::StructPointer { .. })
        || !helper_else.is_empty()
    {
        return None;
    }

    let [Statement::Return(Some(Expression::CallThrough {
        target,
        arguments: callback_arguments,
    }))] = helper_success.as_slice()
    else {
        return None;
    };
    let Expression::Member {
        base: callback_owner,
        offset: callback_offset,
        member_type: callback_type,
        index_stride: None,
    } = target.as_ref()
    else {
        return None;
    };
    let (callback_type_offset, callback_owner_type) = member(callback_owner, &payload.name)?;
    if !matches!(callback_type, Type::Pointer(_) | Type::StructPointer { .. })
        || callback_type_offset != type_offset
        || callback_owner_type != type_member_type
        || !matches!(
            callback_arguments.as_slice(),
            [object_argument, event_argument, trailing_argument]
                if var(object_argument, &object.name)
                    && var(event_argument, &event.name)
                    && var(trailing_argument, &argument.name)
        )
    {
        return None;
    }

    Some(InlinedPayloadEvent {
        helper: helper.clone(),
        registry: registry.clone(),
        test_callee: test_callee.clone(),
        header_size,
        type_offset,
        callback_offset: i16::try_from(*callback_offset).ok()?,
    })
}
