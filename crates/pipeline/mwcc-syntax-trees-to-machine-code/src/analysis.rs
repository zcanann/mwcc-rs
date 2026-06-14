//! Pure predicates and shape queries over expressions — no `Generator` state.

use mwcc_syntax_trees::{BinaryOperator, Expression, Type, UnaryOperator};

pub(crate) fn is_complex(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary { .. } | Expression::Unary { .. } | Expression::Conditional { .. } | Expression::Cast { .. }
    )
}

pub(crate) fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

/// The integer value if `expression` is a literal or a negated literal.
pub(crate) fn constant_value(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value),
        Expression::Unary { operator: UnaryOperator::Negate, operand } => match operand.as_ref() {
            Expression::IntegerLiteral(value) => Some(-*value),
            _ => None,
        },
        _ => None,
    }
}

/// The variable name if `expression` is a plain variable reference.
pub(crate) fn leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

/// The variable name if `expression` is `~variable`.
pub(crate) fn complemented_leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Unary { operator: UnaryOperator::BitNot, operand } => leaf_name(operand),
        _ => None,
    }
}

/// A nonzero integer literal that fits a signed 16-bit immediate.
pub(crate) fn as_small_integer(expression: &Expression) -> Option<i16> {
    match expression {
        Expression::IntegerLiteral(value) if *value != 0 => i16::try_from(*value).ok(),
        _ => None,
    }
}

/// The `(BO, BI)` of the branch that fires when `operator` is **true** (cr0 bits:
/// 0=LT, 1=GT, 2=EQ; BO 12 = if-true, 4 = if-false). The negated branch is
/// `(BO ^ 8, BI)`.
pub(crate) fn positive_branch(operator: BinaryOperator) -> (u8, u8) {
    match operator {
        BinaryOperator::Greater => (12, 1),
        BinaryOperator::Less => (12, 0),
        BinaryOperator::GreaterEqual => (4, 0),
        BinaryOperator::LessEqual => (4, 1),
        BinaryOperator::Equal => (12, 2),
        BinaryOperator::NotEqual => (4, 2),
        _ => (12, 2),
    }
}

/// The logical negation of a comparison operator (`==`↔`!=`, `<`↔`>=`, `>`↔`<=`).
pub(crate) fn flip_comparison(operator: BinaryOperator) -> Option<BinaryOperator> {
    Some(match operator {
        BinaryOperator::Equal => BinaryOperator::NotEqual,
        BinaryOperator::NotEqual => BinaryOperator::Equal,
        BinaryOperator::Less => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::Less,
        BinaryOperator::Greater => BinaryOperator::LessEqual,
        BinaryOperator::LessEqual => BinaryOperator::Greater,
        _ => return None,
    })
}

pub(crate) fn is_comparison(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
    )
}

/// If `expression` is a multiplication, return its two operands.
pub(crate) fn as_multiplication(expression: &Expression) -> Option<(&Expression, &Expression)> {
    match expression {
        Expression::Binary { operator: BinaryOperator::Multiply, left, right } => Some((left, right)),
        _ => None,
    }
}

pub(crate) fn is_commutative(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Add | BinaryOperator::Multiply | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor
    )
}

pub(crate) fn fits_signed_16(value: i64) -> bool {
    (-0x8000..=0x7fff).contains(&value)
}

pub(crate) fn fits_unsigned_16(value: i64) -> bool {
    (0..=0xffff).contains(&value)
}

/// If `value` is a single contiguous run of set bits, return the PowerPC
/// `(mask_begin, mask_end)` for `rlwinm rA,rS,0,begin,end`.
pub(crate) fn contiguous_mask(value: i64) -> Option<(u8, u8)> {
    let mask = value as u32;
    if mask == 0 {
        return None;
    }
    let lowest = mask.trailing_zeros();
    let highest = 31 - mask.leading_zeros();
    let shifted = mask >> lowest;
    if shifted & shifted.wrapping_add(1) != 0 {
        return None; // not a single contiguous run
    }
    Some(((31 - highest) as u8, (31 - lowest) as u8))
}

/// Whether evaluating `expression` uses the scratch register at all — true when
/// any binary node has a binary child.
pub(crate) fn needs_scratch(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => {
            is_complex(left) || is_complex(right) || needs_scratch(left) || needs_scratch(right)
        }
        Expression::Unary { operator, operand } => {
            matches!(operator, UnaryOperator::LogicalNot) || needs_scratch(operand)
        }
        Expression::Conditional { .. } => true,
        Expression::Cast { .. } => true,
        _ => false,
    }
}

/// Whether a type is a narrow integer (sub-32-bit), whose values are extended
/// when read and truncated when produced as a result.
pub(crate) fn is_narrow_int(value_type: Type) -> bool {
    matches!(value_type, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
}

/// Whether `evaluate_*` can compute `expression` into `destination` using only
/// that register and the scratch register.
pub(crate) fn fits_single_scratch(expression: &Expression, destination_is_scratch: bool) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => match (is_complex(left), is_complex(right)) {
            (false, false) => true,
            (true, false) => fits_single_scratch(left, true),
            (false, true) => fits_single_scratch(right, true),
            (true, true) => {
                !destination_is_scratch && fits_single_scratch(left, false) && fits_single_scratch(right, true)
            }
        },
        Expression::Unary { operator, operand } => match operator {
            UnaryOperator::LogicalNot => !destination_is_scratch && fits_single_scratch(operand, destination_is_scratch),
            _ => fits_single_scratch(operand, destination_is_scratch),
        },
        // conditionals and casts are only handled at the top of an evaluation,
        // not nested inside the single-scratch tree model
        Expression::Conditional { .. } | Expression::Cast { .. } => false,
        _ => true,
    }
}
