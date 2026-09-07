//! Prove a selected-bank command followed by word-at-a-time transfer calls.
//! Every loop effect is accounted for, including pointer updates and the
//! signed remaining-byte clamp. Source command order remains available to
//! profiles that do not fuse its mask and shift.
use super::*;
use recognize::{field, integer_constant, named, pointer_casts, poll, slot};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy)]
pub(super) struct Command {
    pub shift: u8,
    pub mask: (u8, u8),
    pub fused_mask: (u8, u8),
    pub shift_first: bool,
    pub high: u16,
}

fn word_casts(mut value: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = value
    {
        value = operand;
    }
    value
}

fn command(value: &Expression, parameter: &str) -> Option<Command> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = word_casts(value)
    else {
        return None;
    };
    let bits = u32::try_from(integer_constant(right)?).ok()?;
    if bits & 0xffff != 0 {
        return None;
    }
    let Expression::Binary {
        operator,
        left,
        right,
    } = word_casts(left)
    else {
        return None;
    };
    let (shift, mask, shift_first) = match operator {
        BinaryOperator::BitAnd => (
            field(word_casts(left), BinaryOperator::ShiftLeft, parameter)?,
            integer_constant(right)?,
            true,
        ),
        BinaryOperator::ShiftLeft => (
            integer_constant(right)?,
            field(word_casts(left), BinaryOperator::BitAnd, parameter)?,
            false,
        ),
        _ => return None,
    };
    let shift = u8::try_from(shift).ok().filter(|s| *s < 32)?;
    let mask = u32::try_from(mask).ok()?;
    let fused = if shift_first {
        mask & (u32::MAX << shift)
    } else {
        mask << shift
    };
    Some(Command {
        shift,
        mask: rlwinm_mask(i64::from(mask))?,
        fused_mask: rlwinm_mask(i64::from(fused))?,
        shift_first,
        high: (bits >> 16) as u16,
    })
}

fn call<'a>(
    statement: &'a Statement,
    error: &str,
    include_previous: bool,
) -> Option<(&'a str, &'a [Expression])> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    if name != error {
        return None;
    }
    let value = if include_previous {
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left,
            right,
        } = value
        else {
            return None;
        };
        if !named(left, error) {
            return None;
        }
        right.as_ref()
    } else {
        value
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = value
    else {
        return None;
    };
    let Expression::Call { name, arguments } = operand.as_ref() else {
        return None;
    };
    Some((name, arguments))
}

fn frame_call<'a>(
    statement: &'a Statement,
    error: &str,
    include_previous: bool,
    local: &str,
    writing: bool,
) -> Option<&'a str> {
    let (target, arguments) = call(statement, error, include_previous)?;
    let [data, length, mode] = arguments else {
        return None;
    };
    let Expression::AddressOf { operand } = pointer_casts(data) else {
        return None;
    };
    (named(operand, local)
        && integer_constant(length) == Some(4)
        && integer_constant(mode) == Some(i64::from(writing)))
    .then_some(target)
}

fn stepped_word(value: &Expression, pointer: &str) -> bool {
    matches!(value, Expression::Dereference { pointer: address }
        if matches!(address.as_ref(), Expression::PostStep { target, operator: BinaryOperator::Add, pointer_link: None }
            if named(target, pointer)))
}

pub(super) fn transaction<'a>(
    function: &'a Function,
    banks: &HashMap<String, (u32, Type)>,
) -> Option<Transaction<'a>> {
    let [command_parameter, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if command_parameter.parameter_type != Type::UnsignedInt
        || data.parameter_type != Type::Pointer(mwcc_syntax_trees::Pointee::UnsignedInt)
        || count.parameter_type != Type::Int
        || function.return_type != Type::Int
        || !function.guards.is_empty()
        || !function.inline_asm_blocks.is_empty()
        || function.locals.len() != 5
        || function.locals.iter().any(|local| {
            local.is_volatile
                || local.is_static
                || local.array_length.is_some()
                || local.attribute_alignment.is_some()
                || local.data_bytes.is_some()
                || !local.data_relocations.is_empty()
                || local.is_const
        })
    {
        return None;
    }
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let Expression::Variable(error) = operand.as_ref() else {
        return None;
    };
    let local = |name: &str| function.locals.iter().find(|local| local.name == name);
    let error_local = local(error)?;
    if error_local.declared_type != Type::Int
        || !matches!(error_local.initializer, Some(Expression::IntegerLiteral(0)))
    {
        return None;
    }
    let [Statement::Assign {
        name: selected_value,
        value: mask,
    }, Statement::Assign {
        name: inserted,
        value: bits,
    }, Statement::Store {
        target,
        value: stored,
    }, Statement::Assign {
        name: command_local,
        value: command_value,
    }, first_call, first_poll, Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }, Statement::Store {
        target: reset_target,
        value: reset_value,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if selected_value != inserted
        || !named(stored, selected_value)
        || field(condition, BinaryOperator::NotEqual, &count.name) != Some(0)
    {
        return None;
    }
    let (bank, selected_index) = slot(target)?;
    let selected_local = local(selected_value)?;
    let &(address, Type::UnsignedInt) = banks.get(bank)? else {
        return None;
    };
    if selected_local.declared_type != Type::UnsignedInt
        || slot(selected_local.initializer.as_ref()?)? != (bank, selected_index)
    {
        return None;
    }
    let preserve = u16::try_from(field(mask, BinaryOperator::BitAnd, selected_value)?).ok()?;
    let insert = u16::try_from(field(bits, BinaryOperator::BitOr, selected_value)?).ok()?;
    let command = command(command_value, &command_parameter.name)?;
    let first_target = frame_call(first_call, error, true, command_local, true)
        .or_else(|| frame_call(first_call, error, false, command_local, true))?;
    let (poll_bank, poll_index, poll_begin, poll_end) = poll(first_poll)?;
    if poll_bank != bank || poll_index == selected_index {
        return None;
    }
    let cursor = function.locals.iter().find(|local| {
        local.declared_type == data.parameter_type
            && local
                .initializer
                .as_ref()
                .is_some_and(|v| named(pointer_casts(v), &data.name))
    })?;
    let [transfer_steps @ .., Statement::Assign {
        name: remaining,
        value: decrement,
    }, Statement::If {
        condition: clamp,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if remaining != &count.name
        || field(decrement, BinaryOperator::Subtract, remaining) != Some(4)
        || field(clamp, BinaryOperator::Less, remaining) != Some(0)
        || !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value: Expression::IntegerLiteral(0) }] if name == remaining)
    {
        return None;
    }
    let (word_name, writing, second_call, second_poll) = match transfer_steps {
        [call, poll, Statement::Store {
            target,
            value: Expression::Variable(word),
        }] if stepped_word(target, &cursor.name) => (word, false, call, poll),
        [Statement::Assign { name: word, value }, call, poll]
            if stepped_word(value, &cursor.name) =>
        {
            (word, true, call, poll)
        }
        _ => return None,
    };
    if frame_call(second_call, error, true, word_name, writing)? != first_target
        || poll(second_poll)? != (poll_bank, poll_index, poll_begin, poll_end)
    {
        return None;
    }
    let roles = [
        error.as_str(),
        selected_value,
        command_local,
        word_name,
        cursor.name.as_str(),
    ];
    if roles.iter().copied().collect::<HashSet<_>>().len() != 5
        || function
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .collect::<HashSet<_>>()
            .len()
            != 5
        || function
            .parameters
            .iter()
            .any(|parameter| roles.contains(&parameter.name.as_str()))
    {
        return None;
    }
    for name in [command_local, word_name] {
        let local = local(name)?;
        if local.declared_type != Type::UnsignedInt || local.initializer.is_some() {
            return None;
        }
    }
    let Expression::IndexedUpdateValue { value } = reset_value else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    if slot(reset_target)? != (bank, selected_index)
        || slot(left)? != (bank, selected_index)
        || integer_constant(right) != Some(i64::from(preserve))
    {
        return None;
    }
    let selected = i16::try_from(selected_index.checked_mul(4)?).ok()?;
    let poll = i16::try_from(poll_index.checked_mul(4)?).ok()?;
    let (_, low) = crate::expressions::split_address(address);
    low.checked_add(selected)?;
    Some(Transaction {
        address,
        selected,
        poll,
        preserve,
        insert,
        poll_begin,
        poll_end,
        transfer: first_target,
        payload: Payload::Stream { command, writing },
    })
}
