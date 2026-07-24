//! Structural recognition for paired nibble-field switch arms.

#[allow(unused_imports)]
use super::super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) struct ScaleSwitch<'a> {
    pub(super) selector: &'a str,
    pub(super) first_scale: &'a str,
    pub(super) second_scale: &'a str,
    pub(super) global: &'a str,
    pub(super) first_offset: i16,
    pub(super) second_offset: i16,
    pub(super) flag_offset: i16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if constant_value(operand) == Some(0))
}

fn one_iteration_body(statement: &Statement) -> Option<&[Statement]> {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    else {
        return None;
    };
    (constant_value(condition) == Some(0)).then_some(body)
}

fn update<'a>(
    statement: &'a Statement,
    preserve: u32,
    shift: i64,
) -> Option<(&'a str, u32, &'a Expression)> {
    let [Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    }] = one_iteration_body(statement)?
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old,
        right: mask,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(stripped(old), Expression::Member {
        base, offset: old_offset, member_type: Type::UnsignedInt, index_stride: None
    } if old_offset == offset
        && matches!(base.as_ref(), Expression::Variable(name) if name == global))
        || constant_value(mask).map(|value| value as u32) != Some(preserve)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: inserted,
        right: found_shift,
    } = right.as_ref()
    else {
        return None;
    };
    (constant_value(found_shift) == Some(shift))
        .then_some((global, *offset, stripped(inserted)))
}

fn fixed_port_write(statement: &Statement, global: &str, offset: u32) -> bool {
    let Some(body) = one_iteration_body(statement) else {
        return false;
    };
    let [
        Statement::Store {
            target: command_target,
            value: command,
        },
        Statement::Store {
            target: data_target,
            value: data,
        },
    ] = body
    else {
        return false;
    };
    let port_target = |target: &Expression, expected_type| {
        matches!(target, Expression::Member {
            base,
            offset: 0,
            member_type,
            index_stride: None,
        } if *member_type == expected_type
            && matches!(stripped(base), Expression::IntegerLiteral(value)
                if *value as u32 == 0xcc00_8000))
    };
    port_target(command_target, Type::UnsignedChar)
        && constant_value(command) == Some(0x61)
        && port_target(data_target, Type::UnsignedInt)
        && matches!(stripped(data), Expression::Member {
            base,
            offset: found_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } if *found_offset == offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == global))
}

fn arm<'a>(
    arm: &'a mwcc_syntax_trees::SwitchArm,
    first_scale: &str,
    second_scale: &str,
    expected_value: i64,
    offset: u32,
    first_shift: i64,
    command: i64,
) -> Option<&'a str> {
    if arm.value != expected_value || arm.falls_through {
        return None;
    }
    let ArmBody::Statements(body) = &arm.body else {
        return None;
    };
    let [first, second, stamp, port] = body.as_slice() else {
        return None;
    };
    let first_mask = !((0xfu32) << first_shift);
    let second_shift = first_shift + 4;
    let second_mask = !((0xfu32) << second_shift);
    let (global, found_offset, first_value) =
        update(first, first_mask, first_shift)?;
    let (second_global, second_offset, second_value) =
        update(second, second_mask, second_shift)?;
    let (stamp_global, stamp_offset, stamp_value) =
        update(stamp, 0x00ff_ffff, 24)?;
    if found_offset != offset
        || second_offset != offset
        || stamp_offset != offset
        || second_global != global
        || stamp_global != global
        || !matches!(first_value, Expression::Variable(name) if name == first_scale)
        || !matches!(second_value, Expression::Variable(name) if name == second_scale)
        || constant_value(stamp_value) != Some(command)
        || !fixed_port_write(port, global, offset)
    {
        return None;
    }
    Some(global)
}

pub(super) fn recognize(function: &Function) -> Option<ScaleSwitch<'_>> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [selector, first_scale, second_scale] = function.parameters.as_slice() else {
        return None;
    };
    if function
        .parameters
        .iter()
        .any(|parameter| parameter.parameter_type != Type::Int)
    {
        return None;
    }
    let [noop, Statement::Switch {
        scrutinee: Expression::Variable(scrutinee),
        arms,
        default,
    }, flag] = function.statements.as_slice()
    else {
        return None;
    };
    if !no_op(noop)
        || scrutinee != &selector.name
        || arms.len() != 4
        || !matches!(default, Some(ArmBody::Statements(body)) if body.is_empty())
    {
        return None;
    }
    let global = arm(
        &arms[0],
        &first_scale.name,
        &second_scale.name,
        0,
        296,
        0,
        0x25,
    )?;
    for (index, offset, shift, command) in
        [(1usize, 296, 8, 0x25), (2, 300, 0, 0x26), (3, 300, 8, 0x26)]
    {
        if arm(
            &arms[index],
            &first_scale.name,
            &second_scale.name,
            index as i64,
            offset,
            shift,
            command,
        )? != global
        {
            return None;
        }
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            },
        value,
    } = flag
    else {
        return None;
    };
    if constant_value(value) != Some(0)
        || !matches!(base.as_ref(), Expression::Variable(name) if name == global)
    {
        return None;
    }
    Some(ScaleSwitch {
        selector: &selector.name,
        first_scale: &first_scale.name,
        second_scale: &second_scale.name,
        global,
        first_offset: 296,
        second_offset: 300,
        flag_offset: i16::try_from(*offset).ok()?,
    })
}
