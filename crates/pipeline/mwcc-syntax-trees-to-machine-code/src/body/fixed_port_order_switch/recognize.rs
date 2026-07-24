//! Structural recognition for paired three-bit packed-field switch arms.

#[allow(unused_imports)]
use super::super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) struct OrderSwitch<'a> {
    pub(super) selector: &'a str,
    pub(super) coordinate: &'a str,
    pub(super) map: &'a str,
    pub(super) global: &'a str,
    pub(super) word_offset: i16,
    pub(super) dirty_offset: i16,
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
    (constant_value(found_shift) == Some(shift)).then_some((global, *offset, stripped(inserted)))
}

fn arm<'a>(
    arm: &'a mwcc_syntax_trees::SwitchArm,
    coordinate: &str,
    map: &str,
    expected_value: i64,
) -> Option<(&'a str, u32)> {
    if arm.value != expected_value || arm.falls_through {
        return None;
    }
    let ArmBody::Statements(body) = &arm.body else {
        return None;
    };
    let [map_update, coordinate_update] = body.as_slice() else {
        return None;
    };
    let map_shift = expected_value * 6;
    let coordinate_shift = map_shift + 3;
    let (global, offset, map_value) = update(map_update, !(0x7u32 << map_shift), map_shift)?;
    let (coordinate_global, coordinate_offset, coordinate_value) = update(
        coordinate_update,
        !(0x7u32 << coordinate_shift),
        coordinate_shift,
    )?;
    if coordinate_global != global
        || coordinate_offset != offset
        || !matches!(map_value, Expression::Variable(name) if name == map)
        || !matches!(coordinate_value, Expression::Variable(name) if name == coordinate)
    {
        return None;
    }
    Some((global, offset))
}

fn fixed_port_write(statement: &Statement, global: &str, offset: u32) -> bool {
    let Some(body) = one_iteration_body(statement) else {
        return false;
    };
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

fn dirty_update(statement: &Statement, global: &str) -> Option<u32> {
    let Statement::Store {
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
    } = statement
    else {
        return None;
    };
    if constant_value(right) != Some(3)
        || !matches!(base.as_ref(), Expression::Variable(name) if name == global)
        || !matches!(left.as_ref(), Expression::Member {
            base,
            offset: old_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } if old_offset == offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == global))
    {
        return None;
    }
    Some(*offset)
}

fn zero_flag(statement: &Statement, global: &str) -> Option<u32> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            },
        value,
    } = statement
    else {
        return None;
    };
    (constant_value(value) == Some(0)
        && matches!(base.as_ref(), Expression::Variable(name) if name == global))
    .then_some(*offset)
}

pub(super) fn recognize(function: &Function) -> Option<OrderSwitch<'_>> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [selector, coordinate, map] = function.parameters.as_slice() else {
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
    }, port, dirty, flag] = function.statements.as_slice()
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
    let (global, word_offset) = arm(&arms[0], &coordinate.name, &map.name, 0)?;
    for (index, candidate) in arms.iter().enumerate().skip(1) {
        let (candidate_global, candidate_offset) =
            arm(candidate, &coordinate.name, &map.name, index as i64)?;
        if candidate_global != global || candidate_offset != word_offset {
            return None;
        }
    }
    if !fixed_port_write(port, global, word_offset) {
        return None;
    }
    Some(OrderSwitch {
        selector: &selector.name,
        coordinate: &coordinate.name,
        map: &map.name,
        global,
        word_offset: i16::try_from(word_offset).ok()?,
        dirty_offset: i16::try_from(dirty_update(dirty, global)?).ok()?,
        flag_offset: i16::try_from(zero_flag(flag, global)?).ok()?,
    })
}
