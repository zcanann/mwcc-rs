//! Source proof for a three-phase, interrupt-protected transport retry.

use super::super::recognize::{integer_constant, named, pointer_casts};
use super::*;

#[cfg(test)]
mod tests;

pub(super) struct RetryTransport<'a> {
    pub acquire: &'a str,
    pub release: &'a str,
    pub status: &'a str,
    pub stream: &'a str,
    pub mailbox: &'a str,
    pub counter: &'a str,
    pub busy_mask: (u8, u8),
    pub counter_mask: (u8, u8),
    pub offset: i16,
    pub stream_base: u32,
    pub rounding_bias: i16,
    pub rounding_mask: (u8, u8),
    pub sequence_shift: u8,
    pub message_tag: u16,
    pub result: i16,
}

pub(super) fn transport(function: &Function) -> Option<RetryTransport<'_>> {
    let [data, size] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(data.parameter_type, Type::Pointer(_))
        || size.parameter_type != Type::UnsignedInt
        || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || function.locals.len() != 3
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_volatile
                || local.is_static
                || local.array_length.is_some()
                || local.attribute_alignment.is_some()
                || local.row_bytes.is_some()
                || local.data_bytes.is_some()
                || !local.data_relocations.is_empty()
        })
    {
        return None;
    }
    let [Statement::Assign {
        name: token,
        value:
            Expression::Call {
                name: acquire,
                arguments: acquire_args,
            },
    }, first_poll, Statement::Store {
        target: Expression::Variable(counter),
        value: increment,
    }, Statement::Assign {
        name: value_name,
        value:
            Expression::Conditional {
                condition: select,
                when_true,
                when_false,
                ..
            },
    }, stream_loop, second_poll, Statement::Assign {
        name: message_name,
        value: message,
    }, mailbox_loop, final_poll, Statement::Expression(Expression::Call {
        name: release,
        arguments: release_args,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if !acquire_args.is_empty()
        || !matches!(release_args.as_slice(), [value] if named(value, token))
        || value_name != message_name
        || integer_constant(when_false)? != 0
    {
        return None;
    }
    let (busy, busy_mask, first) = status_poll(first_poll)?;
    let (other_busy, other_mask, second) = status_poll(second_poll)?;
    let (last_busy, last_mask, last) = status_poll(final_poll)?;
    if busy != other_busy
        || busy != last_busy
        || busy_mask != other_mask
        || busy_mask != last_mask
        || token == value_name
        || token == busy
        || value_name == busy
    {
        return None;
    }
    for (name, ty) in [
        (busy, Type::UnsignedInt),
        (value_name.as_str(), Type::UnsignedInt),
        (token.as_str(), Type::Int),
    ] {
        if !function
            .locals
            .iter()
            .any(|local| local.name == name && local.declared_type == ty)
        {
            return None;
        }
    }
    let status = status_call(first, busy)?;
    if status_call(second, busy)? != status {
        return None;
    }
    let (last_status, last_args) = retry_call(last)?;
    if last_status != status || !output_argument(last_args, busy) {
        return None;
    }
    if field(increment, BinaryOperator::Add, counter)? != 1 {
        return None;
    }
    let counter_mask = rlwinm_mask(field(select, BinaryOperator::BitAnd, counter)?)?;
    let offset = i16::try_from(integer_constant(when_true)?).ok()?;
    let (stream, stream_args) = retry_call(stream_loop)?;
    let [command, buffer, length] = stream_args else {
        return None;
    };
    if !named(pointer_casts(buffer), &data.name) {
        return None;
    }
    let stream_base = field(command, BinaryOperator::BitOr, value_name)? as u32;
    // The measured command schedule has a high and a low immediate merge.
    if stream_base >> 16 == 0 || stream_base & 0xffff == 0 {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = length
    else {
        return None;
    };
    let rounding_bias = affine_bias(left, &size.name)?;
    let alignment = rounding_bias.checked_add(1)?;
    if alignment < 2
        || !(alignment as u32).is_power_of_two()
        || integer_constant(right)? as u32 != (-(alignment as i64)) as u32
    {
        return None;
    }
    let rounding_mask = rlwinm_mask(integer_constant(right)?)?;
    let rounding_bias = i16::try_from(rounding_bias).ok()?;
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = message
    else {
        return None;
    };
    if !named(right, &size.name) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    let sequence_shift = u8::try_from(field(left, BinaryOperator::ShiftLeft, counter)?).ok()?;
    let message_tag = integer_constant(right)? as u32;
    if sequence_shift >= 32 || message_tag & 0xffff != 0 {
        return None;
    }
    let (mailbox, mailbox_args) = retry_call(mailbox_loop)?;
    if !matches!(mailbox_args, [value] if named(value, value_name)) {
        return None;
    }
    let names = [
        acquire.as_str(),
        release.as_str(),
        status,
        stream,
        mailbox,
        counter.as_str(),
    ];
    if names.iter().any(|name| {
        function.locals.iter().any(|local| local.name == *name)
            || function
                .parameters
                .iter()
                .any(|parameter| parameter.name == *name)
    }) {
        return None;
    }
    Some(RetryTransport {
        acquire,
        release,
        status,
        stream,
        mailbox,
        counter,
        busy_mask,
        counter_mask,
        offset,
        stream_base,
        rounding_bias,
        rounding_mask,
        sequence_shift,
        message_tag: (message_tag >> 16) as u16,
        result: i16::try_from(integer_constant(function.return_expression.as_ref()?)?).ok()?,
    })
}

fn field(value: &Expression, operator: BinaryOperator, name: &str) -> Option<i64> {
    super::super::recognize::field(value, operator, name)
}

fn status_poll(statement: &Statement) -> Option<(&str, (u8, u8), &Statement)> {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition:
            Some(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            }),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    let Expression::Variable(busy) = left.as_ref() else {
        return None;
    };
    let [call] = body.as_slice() else { return None };
    Some((busy, rlwinm_mask(integer_constant(right)?)?, call))
}
fn output_argument(arguments: &[Expression], busy: &str) -> bool {
    matches!(arguments, [Expression::AddressOf { operand }] if named(operand, busy))
}
fn status_call<'a>(statement: &'a Statement, busy: &str) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    output_argument(arguments, busy).then_some(name)
}
fn retry_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition:
            Some(Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            }),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    let Expression::Call { name, arguments } = operand.as_ref() else {
        return None;
    };
    body.is_empty().then_some((name, arguments))
}
fn affine_bias(value: &Expression, name: &str) -> Option<i64> {
    if named(value, name) {
        return Some(0);
    }
    let Expression::Binary {
        operator,
        left,
        right,
    } = value
    else {
        return None;
    };
    let bias = affine_bias(left, name)?;
    let delta = integer_constant(right)?;
    match operator {
        BinaryOperator::Add => bias.checked_add(delta),
        BinaryOperator::Subtract => bias.checked_sub(delta),
        _ => None,
    }
}
