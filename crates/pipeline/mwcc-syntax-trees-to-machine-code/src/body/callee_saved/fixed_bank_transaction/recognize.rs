use super::*;
use std::collections::HashMap;

// A transaction constant must require no runtime evaluation. The general
// constant query also knows identities such as x - x; that is insufficient
// here because dropping fixed-bank reads could remove observable effects.
pub(super) fn integer_constant(value: &Expression) -> Option<i64> {
    match value {
        Expression::IntegerLiteral(_) => constant_value(value),
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. }
            if integer_constant(operand).is_some() =>
        {
            constant_value(value)
        }
        Expression::Binary { left, right, .. }
            if integer_constant(left).is_some() && integer_constant(right).is_some() =>
        {
            constant_value(value)
        }
        _ => None,
    }
}

pub(super) fn named(value: &Expression, name: &str) -> bool {
    matches!(value, Expression::Variable(read) if read == name)
}

pub(super) fn field(value: &Expression, operator: BinaryOperator, name: &str) -> Option<i64> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = value
    else {
        return None;
    };
    (*actual == operator && named(left, name))
        .then(|| integer_constant(right))
        .flatten()
}

pub(super) fn slot(value: &Expression) -> Option<(&str, i64)> {
    let Expression::Index { base, index } = value else {
        return None;
    };
    let Expression::Variable(bank) = base.as_ref() else {
        return None;
    };
    Some((bank, integer_constant(index)?))
}

pub(super) fn pointer_casts(mut value: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Pointer(_),
        operand,
    } = value
    {
        value = operand;
    }
    value
}

pub(super) fn accumulated_call<'a>(
    statement: &'a Statement,
    error: &str,
) -> Option<(&'a str, &'a [Expression])> {
    let Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    } = statement
    else {
        return None;
    };
    if name != error || !named(left, error) {
        return None;
    }
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = right.as_ref()
    else {
        return None;
    };
    let Expression::Call { name, arguments } = operand.as_ref() else {
        return None;
    };
    Some((name, arguments))
}

pub(super) fn poll(statement: &Statement) -> Option<(&str, i64, u8, u8)> {
    let Statement::Loop {
        kind: LoopKind::While,
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
    if !body.is_empty() {
        return None;
    }
    let (bank, index) = slot(left)?;
    let mask = integer_constant(right)?;
    if mask == 0 {
        return None;
    }
    let (begin, end) = rlwinm_mask(mask)?;
    Some((bank, index, begin, end))
}

pub(super) fn transaction<'a>(
    function: &'a Function,
    banks: &HashMap<String, (u32, Type)>,
) -> Option<Transaction<'a>> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !function.guards.is_empty()
        || function.parameters.len() != 1
        || function.locals.len() != 3
        || !function.inline_asm_blocks.is_empty()
        || function.locals.iter().any(|local| {
            local.is_volatile
                || local.is_static
                || local.array_length.is_some()
                || local.attribute_alignment.is_some()
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
    let error_local = function.locals.iter().find(|local| local.name == *error)?;
    if !matches!(error_local.declared_type, Type::Int | Type::UnsignedInt)
        || !matches!(error_local.initializer, Some(Expression::IntegerLiteral(0)))
    {
        return None;
    }
    let [Statement::Assign {
        name: temporary,
        value: mask,
    }, Statement::Assign {
        name: inserted,
        value: bits,
    }, Statement::Store {
        target,
        value: stored,
    }, Statement::Assign {
        name: payload_name,
        value: payload_value,
    }, first_call, first_poll, suffix @ ..] = function.statements.as_slice()
    else {
        return None;
    };
    if temporary != inserted
        || !named(stored, temporary)
        || temporary == error
        || payload_name == error
        || payload_name == temporary
    {
        return None;
    }
    let selected_local = function
        .locals
        .iter()
        .find(|local| local.name == *temporary)?;
    let payload_local = function
        .locals
        .iter()
        .find(|local| local.name == *payload_name)?;
    if selected_local.declared_type != Type::UnsignedInt
        || payload_local.declared_type != Type::UnsignedInt
        || payload_local.initializer.is_some()
    {
        return None;
    }
    let (bank, selected) = slot(target)?;
    if slot(selected_local.initializer.as_ref()?)? != (bank, selected) {
        return None;
    }
    let &(address, Type::UnsignedInt) = banks.get(bank)? else {
        return None;
    };
    let preserve = u16::try_from(field(mask, BinaryOperator::BitAnd, temporary)?).ok()?;
    let insert = u16::try_from(field(bits, BinaryOperator::BitOr, temporary)?).ok()?;
    let (transfer, arguments) = accumulated_call(first_call, error)?;
    let [data, length, mode] = arguments else {
        return None;
    };
    let Expression::AddressOf { operand } = pointer_casts(data) else {
        return None;
    };
    if !named(operand, payload_name) || integer_constant(mode) != Some(1) {
        return None;
    }
    let (poll_bank, poll_index, poll_begin, poll_end) = poll(first_poll)?;
    if poll_bank != bank || poll_index == selected {
        return None;
    }
    let parameter = &function.parameters[0];
    let (payload, reset) = match suffix {
        [reset]
            if parameter.parameter_type == Type::UnsignedInt
                && integer_constant(length) == Some(4) =>
        {
            let Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            } = payload_value
            else {
                return None;
            };
            let mask = field(left, BinaryOperator::BitAnd, &parameter.name)?;
            let (begin, end) = rlwinm_mask(mask)?;
            let bits = u32::try_from(integer_constant(right)?).ok()?;
            if bits & 0xffff != 0 {
                return None;
            }
            (
                Payload::Write {
                    begin,
                    end,
                    high: (bits >> 16) as u16,
                },
                reset,
            )
        }
        [second_call, second_poll, reset]
            if matches!(parameter.parameter_type, Type::Pointer(_))
                && integer_constant(length) == Some(2) =>
        {
            let (second_target, arguments) = accumulated_call(second_call, error)?;
            let [data, length, mode] = arguments else {
                return None;
            };
            if second_target != transfer
                || !named(pointer_casts(data), &parameter.name)
                || integer_constant(length) != Some(4)
                || integer_constant(mode) != Some(0)
                || poll(second_poll)? != (poll_bank, poll_index, poll_begin, poll_end)
            {
                return None;
            }
            let bits = u32::try_from(integer_constant(payload_value)?).ok()?;
            if bits & 0xffff != 0 {
                return None;
            }
            (
                Payload::Read {
                    high: (bits >> 16) as u16,
                },
                reset,
            )
        }
        _ => return None,
    };
    let Statement::Store {
        target: reset_target,
        value,
    } = reset
    else {
        return None;
    };
    if slot(reset_target)? != (bank, selected) {
        return None;
    }
    let Expression::IndexedUpdateValue { value } = value else {
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
    if slot(left)? != (bank, selected) || integer_constant(right) != Some(i64::from(preserve)) {
        return None;
    }
    let selected = i16::try_from(selected.checked_mul(4)?).ok()?;
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
        transfer,
        payload,
    })
}
