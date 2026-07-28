//! Semantic recognition for retained callback-or-fallback resource opening.

use super::*;

pub(super) struct InlinedCallbackOpen {
    pub(super) make: String,
    pub(super) free: String,
    pub(super) callback: String,
    pub(super) fallback: String,
    pub(super) object_type: String,
    pub(super) info_offset: i16,
    pub(super) length_offset: i16,
    pub(super) kind_offset: i16,
    pub(super) size_offset: i16,
    pub(super) data_offset: i16,
}

struct Caller {
    helper: String,
    make: String,
    free: String,
    object_type: String,
    info_offset: i16,
    length_offset: i16,
    kind_offset: i16,
    size_offset: i16,
    data_offset: i16,
}

struct Helper {
    callback: String,
    fallback: String,
    info_offset: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn casted_variable(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => casted_variable(operand, expected),
        _ => variable(expression, expected),
    }
}

fn zero(expression: &Expression) -> bool {
    constant_value(expression) == Some(0)
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn negated_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = expression
    else {
        return None;
    };
    direct_call(operand)
}

fn member_of_owner(expression: &Expression, owner: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(base, owner).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn address_of_member(expression: &Expression, owner: &str) -> Option<i16> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    Some(member_of_owner(operand, owner)?.0)
}

fn return_constant(body: &[Statement], expected: i64) -> bool {
    matches!(
        body,
        [Statement::Return(Some(value))] if constant_value(value) == Some(expected)
    )
}

fn classify_caller(function: &Function) -> Option<Caller> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [output, kind, name] = function.parameters.as_slice() else {
        return None;
    };
    if output.parameter_type != Type::Pointer(Pointee::Pointer)
        || kind.parameter_type != Type::Int
        || name.parameter_type != Type::Pointer(Pointee::Char)
    {
        return None;
    }
    let [make_guard, open_guard, release] = function.statements.as_slice() else {
        return None;
    };

    let Statement::If {
        condition: make_condition,
        then_body: make_failure,
        else_body: make_else,
    } = make_guard
    else {
        return None;
    };
    let (make, make_arguments) = negated_call(make_condition)?;
    let [make_output, make_argument, make_type] = make_arguments else {
        return None;
    };
    let Expression::AddressOf { operand: make_type } = make_type else {
        return None;
    };
    let Expression::Variable(object_type) = make_type.as_ref() else {
        return None;
    };
    if !casted_variable(make_output, &output.name)
        || !zero(make_argument)
        || !return_constant(make_failure, 0)
        || !make_else.is_empty()
    {
        return None;
    }

    let Statement::If {
        condition: open_condition,
        then_body: opened,
        else_body: open_else,
    } = open_guard
    else {
        return None;
    };
    let (helper, helper_arguments) = direct_call(open_condition)?;
    if !matches!(
        helper_arguments,
        [output_argument, name_argument]
            if variable(output_argument, &output.name) && variable(name_argument, &name.name)
    ) || !open_else.is_empty()
    {
        return None;
    }
    let [store_kind, store_size, store_data, success] = opened.as_slice() else {
        return None;
    };
    let Statement::Store {
        target: kind_target,
        value: kind_value,
    } = store_kind
    else {
        return None;
    };
    let (kind_offset, kind_type) = member_of_owner(kind_target, &output.name)?;
    if kind_type != Type::Int || !variable(kind_value, &kind.name) {
        return None;
    }
    let Statement::Store {
        target: size_target,
        value: length_value,
    } = store_size
    else {
        return None;
    };
    let (size_offset, size_type) = member_of_owner(size_target, &output.name)?;
    let (length_offset, length_type) = member_of_owner(length_value, &output.name)?;
    if !matches!(size_type, Type::Int | Type::UnsignedInt)
        || !matches!(length_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }
    let Statement::Store {
        target: data_target,
        value: data_value,
    } = store_data
    else {
        return None;
    };
    let (data_offset, data_type) = member_of_owner(data_target, &output.name)?;
    let info_offset = address_of_member(data_value, &output.name)?;
    if !matches!(data_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !matches!(
            success,
            Statement::Return(Some(value)) if constant_value(value) == Some(1)
        )
    {
        return None;
    }

    let Statement::Expression(release) = release else {
        return None;
    };
    let (free, free_arguments) = direct_call(release)?;
    if !matches!(
        free_arguments,
        [argument] if casted_variable(argument, &output.name)
    ) {
        return None;
    }

    Some(Caller {
        helper: helper.to_owned(),
        make: make.to_owned(),
        free: free.to_owned(),
        object_type: object_type.to_owned(),
        info_offset,
        length_offset,
        kind_offset,
        size_offset,
        data_offset,
    })
}

fn helper_call<'a>(expression: &'a Expression, name: &str, output: &str) -> Option<(&'a str, i16)> {
    let (callee, arguments) = direct_call(expression)?;
    let [name_argument, info_argument] = arguments else {
        return None;
    };
    if !variable(name_argument, name) {
        return None;
    }
    Some((callee, address_of_member(info_argument, output)?))
}

fn classify_helper(function: &Function) -> Option<Helper> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || function.guards.len() != 1
    {
        return None;
    }
    let [output, name] = function.parameters.as_slice() else {
        return None;
    };
    if output.parameter_type != Type::Pointer(Pointee::Pointer)
        || name.parameter_type != Type::Pointer(Pointee::Char)
    {
        return None;
    }
    let guard = &function.guards[0];
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = &guard.condition
    else {
        return None;
    };
    let callback = match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(callback), zero_value) if zero(zero_value) => callback,
        (zero_value, Expression::Variable(callback)) if zero(zero_value) => callback,
        _ => return None,
    };
    let (called_callback, callback_info) = helper_call(&guard.value, &name.name, &output.name)?;
    let (fallback, fallback_info) = helper_call(
        function.return_expression.as_ref()?,
        &name.name,
        &output.name,
    )?;
    if called_callback != callback || callback_info != fallback_info {
        return None;
    }
    Some(Helper {
        callback: callback.to_owned(),
        fallback: fallback.to_owned(),
        info_offset: callback_info,
    })
}

pub(super) fn classify(
    function: &Function,
    inline_bodies: &crate::inline_expansion::InlineBodySet,
) -> Option<InlinedCallbackOpen> {
    let caller = classify_caller(function)?;
    let helper = classify_helper(inline_bodies.retained_body(&caller.helper)?)?;
    if caller.info_offset != helper.info_offset {
        return None;
    }
    Some(InlinedCallbackOpen {
        make: caller.make,
        free: caller.free,
        callback: helper.callback,
        fallback: helper.fallback,
        object_type: caller.object_type,
        info_offset: caller.info_offset,
        length_offset: caller.length_offset,
        kind_offset: caller.kind_offset,
        size_offset: caller.size_offset,
        data_offset: caller.data_offset,
    })
}
