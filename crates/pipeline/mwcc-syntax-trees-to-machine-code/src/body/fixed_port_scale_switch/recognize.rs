//! Structural recognition for paired nibble-field switch arms.

#[allow(unused_imports)]
use super::super::*;
use mwcc_syntax_trees::ArmBody;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScaleUpdate {
    ShiftOr,
    RotateInsert,
}

pub(super) struct ScaleSwitch<'a> {
    pub(super) selector: &'a str,
    pub(super) first_scale: &'a str,
    pub(super) second_scale: &'a str,
    pub(super) global: &'a str,
    pub(super) first_offset: i16,
    pub(super) second_offset: i16,
    pub(super) flag_offset: i16,
    pub(super) update: ScaleUpdate,
}

fn word_value(mut expression: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = expression
    {
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
) -> Option<(&'a str, u32, &'a Expression, ScaleUpdate)> {
    let statement = match one_iteration_body(statement) {
        Some([only]) => only,
        Some(_) => return None,
        None => statement,
    };
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value,
    } = statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    let is_old_member = |old: &Expression| {
        matches!(word_value(old), Expression::Member {
        base, offset: old_offset, member_type: Type::UnsignedInt, index_stride: None,
    } if old_offset == offset
        && matches!(base.as_ref(), Expression::Variable(name) if name == global))
    };
    if let Expression::Call { name, arguments } = word_value(value) {
        if crate::intrinsics::classify(name, arguments.len())
            != Some(crate::intrinsics::Intrinsic::RotateLeftWordInsert)
        {
            return None;
        }
        let insert = crate::intrinsics::rotate_insert(arguments)?;
        let mask = !preserve;
        if !is_old_member(insert.initial)
            || i64::from(insert.shift) != shift
            || u32::from(insert.begin) != mask.leading_zeros()
            || u32::from(insert.end) != 31 - mask.trailing_zeros()
        {
            return None;
        }
        return Some((
            global,
            *offset,
            word_value(insert.source),
            ScaleUpdate::RotateInsert,
        ));
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
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
    if !is_old_member(old) || constant_value(mask).map(|value| value as u32) != Some(preserve) {
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
    (constant_value(found_shift) == Some(shift)).then_some((
        global,
        *offset,
        word_value(inserted),
        ScaleUpdate::ShiftOr,
    ))
}

fn fixed_port_write(body: &[Statement], global: &str, offset: u32) -> bool {
    let [Statement::Store {
        target: command_target,
        value: command,
    }, Statement::Store {
        target: data_target,
        value: data,
    }] = body
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
            && matches!(base.as_ref(), Expression::Cast {
                target_type: Type::StructPointer { .. }, operand,
            } if constant_value(operand).map(|value| value as u32) == Some(0xcc00_8000)))
    };
    port_target(command_target, Type::UnsignedChar)
        && constant_value(command) == Some(0x61)
        && port_target(data_target, Type::UnsignedInt)
        && matches!(word_value(data), Expression::Member {
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
    offset: Option<u32>,
    first_shift: i64,
    command: i64,
) -> Option<(&'a str, u32, ScaleUpdate)> {
    if arm.value != expected_value || arm.falls_through {
        return None;
    }
    let ArmBody::Statements(body) = &arm.body else {
        return None;
    };
    let (first, second, stamp, port) = match body.as_slice() {
        [first, second, stamp, port] => (first, second, stamp, one_iteration_body(port)?),
        [first, second, stamp, port @ ..] if port.len() == 2 => (first, second, stamp, port),
        _ => return None,
    };
    let first_mask = !((0xfu32) << first_shift);
    let second_shift = first_shift + 4;
    let second_mask = !((0xfu32) << second_shift);
    let (global, found_offset, first_value, update_kind) = update(first, first_mask, first_shift)?;
    let (second_global, second_offset, second_value, second_kind) =
        update(second, second_mask, second_shift)?;
    let (stamp_global, stamp_offset, stamp_value, stamp_kind) = update(stamp, 0x00ff_ffff, 24)?;
    if offset.is_some_and(|offset| found_offset != offset)
        || second_offset != found_offset
        || stamp_offset != found_offset
        || update_kind != second_kind
        || update_kind != stamp_kind
        || second_global != global
        || stamp_global != global
        || !matches!(first_value, Expression::Variable(name) if name == first_scale)
        || !matches!(second_value, Expression::Variable(name) if name == second_scale)
        || constant_value(stamp_value) != Some(command)
        || !fixed_port_write(port, global, found_offset)
    {
        return None;
    }
    Some((global, found_offset, update_kind))
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
    let statements = match function.statements.as_slice() {
        [noop, rest @ ..] if no_op(noop) => rest,
        statements => statements,
    };
    let [Statement::Switch {
        scrutinee: Expression::Variable(scrutinee),
        arms,
        default,
    }, flag] = statements
    else {
        return None;
    };
    if scrutinee != &selector.name
        || arms.len() != 4
        || !matches!(default, Some(ArmBody::Statements(body)) if body.is_empty())
    {
        return None;
    }
    let (global, first_offset, update) = arm(
        &arms[0],
        &first_scale.name,
        &second_scale.name,
        0,
        None,
        0,
        0x25,
    )?;
    let (second_global, second_offset, second_update) = arm(
        &arms[2],
        &first_scale.name,
        &second_scale.name,
        2,
        None,
        0,
        0x26,
    )?;
    if second_global != global || second_update != update {
        return None;
    }
    for (index, offset, command) in [(1usize, first_offset, 0x25), (3, second_offset, 0x26)] {
        if arm(
            &arms[index],
            &first_scale.name,
            &second_scale.name,
            index as i64,
            Some(offset),
            8,
            command,
        )? != (global, offset, update)
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
        first_offset: i16::try_from(first_offset).ok()?,
        second_offset: i16::try_from(second_offset).ok()?,
        flag_offset: i16::try_from(*offset).ok()?,
        update,
    })
}
