//! Recognition for counted scalar-read loops around a one-element wrapper.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct ReadLoop<'a> {
    pub(super) callee: ReadLoopCallee<'a>,
    pub(super) width: u8,
}

pub(super) enum ReadLoopCallee<'a> {
    Wrapper(&'a str),
    Core(&'a str),
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn indexed_data_argument(expression: &Expression, data: &str, index: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } | Expression::AddressOf { operand } => {
            indexed_data_argument(operand, data, index)
        }
        Expression::Index {
            base,
            index: call_index,
        } => var(base, data) && var(call_index, index),
        _ => false,
    }
}

pub(super) fn classify_read_loop(function: &Function) -> Option<ReadLoop<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || count.parameter_type != Type::Int
    {
        return None;
    }
    let width = match data.parameter_type {
        Type::Pointer(Pointee::UnsignedChar) => 1,
        Type::Pointer(Pointee::UnsignedInt) => 4,
        Type::Pointer(Pointee::UnsignedLongLong) => 8,
        _ => return None,
    };
    let [error, index] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || error.initializer.is_some()
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer, Expression::Comma { left, right }
        if matches!(left.as_ref(), Expression::Assign { target, value }
            if var(target, &index.name) && constant_value(value) == Some(0))
        && matches!(right.as_ref(), Expression::Assign { target, value }
            if var(target, &error.name) && constant_value(value) == Some(0)))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal,
                left: error_value,
                right: zero,
            } if var(error_value, &error.name) && constant_value(zero) == Some(0))
            && matches!(right.as_ref(), Expression::Binary {
                operator: BinaryOperator::Less,
                left: index_value,
                right: count_value,
            } if var(index_value, &index.name) && var(count_value, &count.name)))
        || !matches!(step, Expression::Assign { target, value }
            if var(target, &index.name)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if var(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_error,
        value: Expression::Call {
            name: wrapper_callee,
            arguments,
        },
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_error != &error.name {
        return None;
    }
    let callee = match arguments.as_slice() {
        [call_buffer, call_data]
            if var(call_buffer, &buffer.name)
                && indexed_data_argument(call_data, &data.name, &index.name) =>
        {
            ReadLoopCallee::Wrapper(wrapper_callee)
        }
        [call_buffer, call_data, call_width]
            if var(call_buffer, &buffer.name)
                && indexed_data_argument(call_data, &data.name, &index.name)
                && constant_value(call_width) == Some(i64::from(width)) =>
        {
            ReadLoopCallee::Core(wrapper_callee)
        }
        _ => return None,
    };
    Some(ReadLoop {
        callee,
        width,
    })
}
