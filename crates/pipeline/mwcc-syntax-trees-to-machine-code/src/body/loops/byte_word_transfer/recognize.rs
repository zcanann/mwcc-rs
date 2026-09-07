use super::*;
use std::collections::{HashMap, HashSet};

fn binary(value: &Expression, op: BinaryOperator) -> Option<(&Expression, &Expression)> {
    match value {
        Expression::Binary {
            operator,
            left,
            right,
        } if *operator == op => Some((left, right)),
        _ => None,
    }
}
fn named(value: &Expression, name: &str) -> bool {
    matches!(value, Expression::Variable(read) if read == name)
}
fn name(value: &Expression) -> Option<&str> {
    match value {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}
// Never discard a runtime expression merely because an algebraic identity
// gives it a constant value. Fixed-bank reads can have observable effects.
fn integer(value: &Expression) -> Option<i64> {
    match value {
        Expression::IntegerLiteral(value) => Some(*value),
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } if integer(operand).is_some() => constant_value(value),
        _ => None,
    }
}
fn slot(value: &Expression) -> Option<(&str, i16)> {
    let Expression::Index { base, index } = value else {
        return None;
    };
    let index = integer(index)?;
    if index < 0 {
        return None;
    }
    Some((name(base)?, i16::try_from(index.checked_mul(4)?).ok()?))
}
fn byte_pointer(value: &Expression, parameter: &str) -> bool {
    matches!(value, Expression::Cast { target_type: Type::Pointer(Pointee::UnsignedChar), operand }
        if named(operand, parameter))
}
fn byte_shift(value: &Expression, index: &str) -> bool {
    let Some((difference, scale)) = binary(value, BinaryOperator::ShiftLeft) else {
        return false;
    };
    let Some((last, iteration)) = binary(difference, BinaryOperator::Subtract) else {
        return false;
    };
    integer(last) == Some(3) && named(iteration, index) && integer(scale) == Some(3)
}
fn counted<'a>(statement: &'a Statement, bound: &str) -> Option<(&'a str, &'a [Statement])> {
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
    let Expression::Assign { target, value } = initializer else {
        return None;
    };
    let index = name(target)?;
    if integer(value) != Some(0) {
        return None;
    }
    let (left, right) = binary(condition, BinaryOperator::Less)?;
    if !named(left, index) || !named(right, bound) {
        return None;
    }
    let Expression::Assign { target, value } = step else {
        return None;
    };
    let (left, right) = binary(value, BinaryOperator::Add)?;
    (named(target, index) && named(left, index) && integer(right) == Some(1))
        .then_some((index, body))
}
fn field_shift(value: &Expression) -> Option<(&Expression, u8)> {
    let (value, shift) = binary(value, BinaryOperator::ShiftLeft)?;
    let shift = u8::try_from(integer(shift)?).ok()?;
    (shift > 0 && shift < 32).then_some((value, shift))
}

pub(super) fn transfer<'a>(
    function: &'a Function,
    banks: &HashMap<String, (u32, Type)>,
) -> Option<Transfer<'a>> {
    let [data, count, mode] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(data.parameter_type, Type::Pointer(_))
        || count.parameter_type != Type::Int
        || mode.parameter_type != Type::UnsignedInt
        || function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.return_expression.as_ref().and_then(integer) != Some(1)
        || !function.inline_asm_blocks.is_empty()
        || function.asm_body.is_some()
        || function.locals.len() != 5
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_volatile
                || local.is_static
                || local.array_length.is_some()
                || local.data_bytes.is_some()
                || !local.data_relocations.is_empty()
                || local.attribute_alignment.is_some()
                || local.row_bytes.is_some()
        })
    {
        return None;
    }
    let [Statement::If {
        condition: writing,
        then_body: pack,
        else_body: pack_else,
    }, Statement::Store {
        target: control_target,
        value: control_value,
    }, poll, Statement::If {
        condition: reading,
        then_body: unpack,
        else_body: unpack_else,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !named(writing, &mode.name)
        || !pack_else.is_empty()
        || !unpack_else.is_empty()
        || !matches!(reading, Expression::Unary { operator: UnaryOperator::LogicalNot, operand }
            if named(operand, &mode.name))
    {
        return None;
    }
    let [Statement::Assign {
        name: packed,
        value: zero,
    }, pack_loop, Statement::Store {
        target: packed_target,
        value: packed_value,
    }] = pack.as_slice()
    else {
        return None;
    };
    if integer(zero) != Some(0) || !named(packed_value, packed) {
        return None;
    }
    let (index, pack_body) = counted(pack_loop, &count.name)?;
    let [Statement::Assign {
        name: pack_pointer,
        value: pointer_value,
    }, Statement::Assign {
        name: updated,
        value: update,
    }] = pack_body
    else {
        return None;
    };
    let (pointer_base, pointer_index) = binary(pointer_value, BinaryOperator::Add)?;
    let (old, shifted) = binary(update, BinaryOperator::BitOr)?;
    let (byte, shift) = binary(shifted, BinaryOperator::ShiftLeft)?;
    if updated != packed
        || !named(old, packed)
        || !byte_pointer(pointer_base, &data.name)
        || !named(pointer_index, index)
        || !byte_shift(shift, index)
        || !matches!(byte, Expression::Dereference { pointer } if named(pointer, pack_pointer))
    {
        return None;
    }
    let [Statement::Assign {
        name: unpack_pointer,
        value: unpack_base,
    }, Statement::Assign {
        name: word,
        value: word_slot,
    }, unpack_loop] = unpack.as_slice()
    else {
        return None;
    };
    if !byte_pointer(unpack_base, &data.name) {
        return None;
    }
    let (read_index, read_body) = counted(unpack_loop, &count.name)?;
    let [Statement::Store { target, value }] = read_body else {
        return None;
    };
    let (read_word, read_shift) = binary(value, BinaryOperator::ShiftRight)?;
    if read_index != index
        || !named(read_word, word)
        || !byte_shift(read_shift, index)
        || !matches!(target, Expression::Dereference { pointer }
            if matches!(pointer.as_ref(), Expression::PostStep { target, operator: BinaryOperator::Add, pointer_link: None }
                if named(target, unpack_pointer)))
    {
        return None;
    }
    let (bank, data_offset) = slot(packed_target)?;
    let (control_bank, control_offset) = slot(control_target)?;
    if slot(word_slot)? != (bank, data_offset)
        || control_bank != bank
        || data_offset == control_offset
    {
        return None;
    }
    let &(address, element) = banks.get(bank)?;
    let (_, low) = crate::expressions::split_address(address);
    if element != Type::UnsignedInt || low.checked_add(data_offset).is_none() {
        return None;
    }
    let (mode_bits, count_bits) = binary(control_value, BinaryOperator::BitOr)?;
    let (start, mode_bits) = binary(mode_bits, BinaryOperator::BitOr)?;
    let start = u16::try_from(integer(start)?).ok()?;
    let (mode_value, mode_shift) = field_shift(mode_bits)?;
    let (count_value, count_shift) = field_shift(count_bits)?;
    let (count_value, one) = binary(count_value, BinaryOperator::Subtract)?;
    if !named(mode_value, &mode.name)
        || !named(count_value, &count.name)
        || integer(one) != Some(1)
        || !start.is_power_of_two()
        || u32::from(start) >= (1u32 << mode_shift)
    {
        return None;
    }
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(poll_condition),
        step: None,
        body,
    } = poll
    else {
        return None;
    };
    let (poll_slot, mask) = binary(poll_condition, BinaryOperator::BitAnd)?;
    if !body.is_empty() || slot(poll_slot)? != (bank, control_offset) {
        return None;
    }
    let mask = u32::try_from(integer(mask)?).ok()?;
    if !mask.is_power_of_two() {
        return None;
    }
    let roles = [
        (packed.as_str(), Type::UnsignedInt),
        (word.as_str(), Type::UnsignedInt),
        (index, Type::Int),
        (pack_pointer.as_str(), Type::Pointer(Pointee::UnsignedChar)),
        (
            unpack_pointer.as_str(),
            Type::Pointer(Pointee::UnsignedChar),
        ),
    ];
    let names: HashSet<_> = roles.iter().map(|(name, _)| *name).collect();
    if names.len() != roles.len()
        || function
            .parameters
            .iter()
            .any(|p| names.contains(p.name.as_str()))
        || roles.iter().any(|(name, kind)| {
            !function
                .locals
                .iter()
                .any(|l| l.name == *name && l.declared_type == *kind)
        })
    {
        return None;
    }
    Some(Transfer {
        address,
        data_offset,
        control_offset,
        start,
        mode_shift,
        count_shift,
        poll_bit: 31 - mask.trailing_zeros() as u8,
        index,
        count: &count.name,
    })
}
