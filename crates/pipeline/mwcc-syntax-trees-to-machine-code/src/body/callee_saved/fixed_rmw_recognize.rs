//! Semantic recognition shared by fixed-address RMW schedules.

#[allow(unused_imports)]
use super::*;

pub(super) fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

pub(super) fn fixed_slot(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::Variable(bank) = base.as_ref() else {
        return None;
    };
    Some((bank, constant_value(index)?))
}

pub(super) fn rmw_parts<'a>(
    target: &Expression,
    value: &'a Expression,
) -> Option<(i64, &'a Expression)> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right: inserted,
    } = peel_casts(value)
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: loaded,
        right: preserve,
    } = peel_casts(left)
    else {
        return None;
    };
    if !same_operand(target, loaded) {
        return None;
    }
    Some((constant_value(preserve)?, inserted.as_ref()))
}

pub(super) fn shifted_name(expression: &Expression, shift: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left,
        right,
    } = peel_casts(expression)
    else {
        return None;
    };
    (constant_value(right) == Some(shift))
        .then(|| leaf_name(left))
        .flatten()
}

pub(super) fn shifted_left_name(expression: &Expression, shift: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left,
        right,
    } = peel_casts(expression)
    else {
        return None;
    };
    (constant_value(right) == Some(shift))
        .then(|| leaf_name(left))
        .flatten()
}

pub(super) fn masked_side_is_narrow(value: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        ..
    } = peel_casts(value)
    else {
        return false;
    };
    matches!(
        left.as_ref(),
        Expression::Cast {
            target_type: Type::UnsignedShort,
            ..
        }
    )
}

pub(super) fn low_half_name(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = peel_casts(expression)
    else {
        return None;
    };
    if constant_value(right) == Some(0xffff) {
        return leaf_name(left);
    }
    if constant_value(left) == Some(0xffff) {
        return leaf_name(right);
    }
    None
}

pub(super) fn shifted_low_half_name(expression: &Expression, shift: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = peel_casts(expression)
    else {
        return None;
    };
    (constant_value(right) == Some(0xffff))
        .then(|| shifted_name(left, shift))
        .flatten()
}
