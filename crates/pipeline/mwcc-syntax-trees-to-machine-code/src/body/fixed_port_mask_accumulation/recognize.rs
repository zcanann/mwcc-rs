//! Structural recognition for counted packed-field mask accumulation.

#[allow(unused_imports)]
use super::super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) struct MaskAccumulation<'a> {
    pub(super) global: &'a str,
    pub(super) count_offset: i16,
    pub(super) packed_offset: i16,
    pub(super) mask_offset: i16,
    pub(super) flag_offset: i16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(stripped(expression), Expression::Variable(name) if name == expected)
}

fn member<'a>(expression: &'a Expression, global: &str) -> Option<(u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = stripped(expression)
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == global)
        .then_some((*offset, *member_type))
}

fn extracted_member(expression: &Expression, mask: i64, shift: i64) -> Option<(&str, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right) != Some(mask) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left: source,
        right: found_shift,
    } = stripped(left)
    else {
        return None;
    };
    if constant_value(found_shift) != Some(shift) {
        return None;
    }
    let Expression::Member {
        base,
        offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = source.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, *offset))
}

fn assigned_zero(statement: &Statement) -> Option<&str> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    (constant_value(value) == Some(0)).then_some(name)
}

fn switch_arm<'a>(
    arm: &'a mwcc_syntax_trees::SwitchArm,
    selected: &str,
    expected_value: i64,
    expected_shift: i64,
) -> Option<(&'a str, u32)> {
    if arm.value != expected_value || arm.falls_through {
        return None;
    }
    let ArmBody::Statements(body) = &arm.body else {
        return None;
    };
    let [Statement::Assign { name, value }] = body.as_slice() else {
        return None;
    };
    (name == selected)
        .then(|| extracted_member(value, 7, expected_shift))
        .flatten()
}

fn one_iteration(statement: &Statement) -> Option<&[Statement]> {
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

pub(super) fn recognize(function: &Function) -> Option<MaskAccumulation<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function.locals.len() != 6
        || function
            .locals
            .iter()
            .any(|local| local.declared_type != Type::UnsignedInt)
    {
        return None;
    }
    let [zero_mask, dead_zero, count_assignment, loop_statement, tail] =
        function.statements.as_slice()
    else {
        return None;
    };
    let accumulated = assigned_zero(zero_mask)?;
    assigned_zero(dead_zero)?;
    let Statement::Assign {
        name: count,
        value: count_value,
    } = count_assignment
    else {
        return None;
    };
    let (global, count_offset) = extracted_member(count_value, 7, 16)?;

    let Statement::Loop {
        kind: LoopKind::For,
        initializer:
            Some(Expression::Assign {
                target: initializer_target,
                value: initializer_value,
            }),
        condition: Some(loop_condition),
        step:
            Some(Expression::Assign {
                target: step_target,
                value: step_value,
            }),
        body,
    } = loop_statement
    else {
        return None;
    };
    let Expression::Variable(index) = initializer_target.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: condition_left,
        right: condition_right,
    } = loop_condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: step_left,
        right: step_right,
    } = step_value.as_ref()
    else {
        return None;
    };
    if constant_value(initializer_value) != Some(0)
        || !variable(condition_left, index)
        || !variable(condition_right, count)
        || !variable(step_target, index)
        || !variable(step_left, index)
        || constant_value(step_right) != Some(1)
    {
        return None;
    }
    let [Statement::Switch {
        scrutinee,
        arms,
        default: None,
    }, Statement::Assign {
        name: accumulated_name,
        value: accumulated_value,
    }] = body.as_slice()
    else {
        return None;
    };
    if !variable(scrutinee, index) || accumulated_name != accumulated || arms.len() != 4 {
        return None;
    }
    let selected = function
        .locals
        .iter()
        .map(|local| local.name.as_str())
        .find(|name| switch_arm(&arms[0], name, 0, 0).is_some())?;
    let (packed_global, packed_offset) = switch_arm(&arms[0], selected, 0, 0)?;
    if packed_global != global {
        return None;
    }
    for (index, shift) in [(1, 6), (2, 12), (3, 18)] {
        if switch_arm(&arms[index], selected, index as i64, shift) != Some((global, packed_offset))
        {
            return None;
        }
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: accumulated_old,
        right: one_hot,
    } = accumulated_value
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: one,
        right: selected_value,
    } = one_hot.as_ref()
    else {
        return None;
    };
    if !variable(accumulated_old, accumulated)
        || constant_value(one) != Some(1)
        || !variable(selected_value, selected)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: current_mask,
                right: computed_mask,
            },
        then_body,
        else_body,
    } = tail
    else {
        return None;
    };
    if !else_body.is_empty() || !variable(computed_mask, accumulated) {
        return None;
    }
    let (mask_offset, Type::UnsignedInt) = member(current_mask, global)? else {
        return None;
    };
    let [update, port, flag] = then_body.as_slice() else {
        return None;
    };
    let [Statement::Store {
        target: update_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: preserved,
                right: inserted,
            },
    }] = one_iteration(update)?
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old_mask,
        right: preserve_mask,
    } = preserved.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: insert_value,
        right: insert_shift,
    } = inserted.as_ref()
    else {
        return None;
    };
    if member(update_target, global) != Some((mask_offset, Type::UnsignedInt))
        || member(old_mask, global) != Some((mask_offset, Type::UnsignedInt))
        || constant_value(preserve_mask).map(|value| value as u32) != Some(0xffff_ff00)
        || !variable(insert_value, accumulated)
        || constant_value(insert_shift) != Some(0)
    {
        return None;
    }
    let [Statement::Store {
        target: command_target,
        value: command,
    }, Statement::Store {
        target: data_target,
        value: data,
    }] = one_iteration(port)?
    else {
        return None;
    };
    let fixed_target = |target: &Expression, expected_type| {
        matches!(target, Expression::Member {
            base,
            offset: 0,
            member_type,
            index_stride: None,
        } if *member_type == expected_type
            && matches!(stripped(base), Expression::IntegerLiteral(value)
                if *value as u32 == 0xcc00_8000))
    };
    let Statement::Store {
        target: flag_target,
        value: flag_value,
    } = flag
    else {
        return None;
    };
    let (flag_offset, Type::UnsignedShort) = member(flag_target, global)? else {
        return None;
    };
    if constant_value(command) != Some(0x61)
        || !fixed_target(command_target, Type::UnsignedChar)
        || !fixed_target(data_target, Type::UnsignedInt)
        || member(data, global) != Some((mask_offset, Type::UnsignedInt))
        || constant_value(flag_value) != Some(0)
    {
        return None;
    }
    Some(MaskAccumulation {
        global,
        count_offset: i16::try_from(count_offset).ok()?,
        packed_offset: i16::try_from(packed_offset).ok()?,
        mask_offset: i16::try_from(mask_offset).ok()?,
        flag_offset: i16::try_from(flag_offset).ok()?,
    })
}
