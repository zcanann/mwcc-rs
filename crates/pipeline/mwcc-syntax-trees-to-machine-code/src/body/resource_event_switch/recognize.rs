//! Semantic recognition for a resource allocation/free event switch.

use super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) struct ResourceEventSwitch {
    pub(super) take: String,
    pub(super) close: String,
    pub(super) free: String,
    pub(super) size_offset: i16,
    pub(super) position_offset: i16,
    pub(super) data_offset: i16,
    pub(super) buffer_offset: i16,
    pub(super) info_offset: i16,
    pub(super) allocation_flags: u32,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn member_of(expression: &Expression, owner: &str) -> Option<(i16, Type)> {
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

fn address_of_member(expression: &Expression, owner: &str) -> Option<(i16, Type)> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    member_of(operand, owner)
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

fn failure_guard(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(
            then_body.as_slice(),
            [Statement::Return(Some(value))] if constant_value(value) == Some(0)
        )
    {
        return None;
    }
    negated_call(condition)
}

fn zero_member_store(statement: &Statement, owner: &str) -> Option<(i16, Type)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    (constant_value(value) == Some(0)).then_some(member_of(target, owner)?)
}

fn statements(arm: &mwcc_syntax_trees::SwitchArm) -> Option<&[Statement]> {
    let ArmBody::Statements(statements) = &arm.body else {
        return None;
    };
    Some(statements)
}

fn empty_arm(arm: &mwcc_syntax_trees::SwitchArm, value: i64, falls_through: bool) -> bool {
    arm.value == value
        && arm.falls_through == falls_through
        && statements(arm).is_some_and(<[Statement]>::is_empty)
}

pub(super) fn classify(function: &Function) -> Option<ResourceEventSwitch> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [object, event, _argument] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::StructPointer { .. } | Type::Pointer(_)
    ) || event.parameter_type != Type::Int
    {
        return None;
    }
    let [Statement::Switch {
        scrutinee,
        arms,
        default: Some(default),
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !variable(scrutinee, &event.name)
        || constant_value(default.return_expression()?) != Some(0)
        || arms.len() != 5
    {
        return None;
    }
    let init = arms.iter().find(|arm| arm.value == 2)?;
    let destroy = arms.iter().find(|arm| arm.value == 3)?;
    if init.falls_through
        || destroy.falls_through
        || !arms.iter().any(|arm| empty_arm(arm, 0, true))
        || !arms.iter().any(|arm| empty_arm(arm, 1, true))
        || !arms.iter().any(|arm| empty_arm(arm, 4, false))
    {
        return None;
    }

    let [size_store, position_store, data_store, take_guard] = statements(init)? else {
        return None;
    };
    let (size_offset, size_type) = zero_member_store(size_store, &object.name)?;
    let (position_offset, position_type) = zero_member_store(position_store, &object.name)?;
    let (data_offset, data_type) = zero_member_store(data_store, &object.name)?;
    if !matches!(size_type, Type::Int | Type::UnsignedInt)
        || !matches!(position_type, Type::Int | Type::UnsignedInt)
        || !matches!(data_type, Type::Pointer(_) | Type::StructPointer { .. })
    {
        return None;
    }
    let (take, take_arguments) = failure_guard(take_guard)?;
    let [buffer, flags] = take_arguments else {
        return None;
    };
    let (buffer_offset, buffer_type) = address_of_member(buffer, &object.name)?;
    if !matches!(buffer_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let allocation_flags = u32::try_from(constant_value(flags)?).ok()?;

    let [Statement::Expression(close_call), free_guard] = statements(destroy)? else {
        return None;
    };
    let (close, close_arguments) = direct_call(close_call)?;
    let [info] = close_arguments else {
        return None;
    };
    let (info_offset, info_type) = address_of_member(info, &object.name)?;
    if !matches!(info_type, Type::Struct { .. }) {
        return None;
    }
    let (free, free_arguments) = failure_guard(free_guard)?;
    let [freed_buffer] = free_arguments else {
        return None;
    };
    if address_of_member(freed_buffer, &object.name)?.0 != buffer_offset {
        return None;
    }

    Some(ResourceEventSwitch {
        take: take.to_owned(),
        close: close.to_owned(),
        free: free.to_owned(),
        size_offset,
        position_offset,
        data_offset,
        buffer_offset,
        info_offset,
        allocation_flags,
    })
}
