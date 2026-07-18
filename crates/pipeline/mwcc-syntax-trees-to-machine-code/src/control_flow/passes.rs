//! Shared select/mask analysis helpers and small types.

#[allow(unused_imports)]
use super::*;

/// A select arm that mwcc materializes into a register with a plain arithmetic instruction (or a
/// short strength-reduced sequence), for which it uses the simple branch select used by the
/// computed-arm handlers above. Comparisons (0/1 idioms), logicals, loads (deref/member/index),
/// calls, and casts use different codegen there, so they are EXCLUDED — intercepting them in the
/// branch select would emit wrong bytes (a latent diff the canary set does not cover). Division
/// and remainder are excluded too (their magic-number sequences are not validated in this form).
pub(crate) fn is_simple_arithmetic_arm(expression: &Expression) -> bool {
    // A constant-valued expression is NOT a computed arm even when its AST is arithmetic — `-1`
    // is `Unary{Negate, 1}` and `3 + 4` folds to `7`. Those belong to the constant-arm handlers,
    // so exclude anything `constant_value` can fold (otherwise `(c) ? -1 : x` is mis-selected).
    if constant_value(expression).is_some() {
        return false;
    }
    match expression {
        Expression::Binary { operator, .. } => matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::ShiftLeft
                | BinaryOperator::ShiftRight
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor
        ),
        Expression::Unary { operator, .. } => {
            matches!(operator, UnaryOperator::Negate | UnaryOperator::BitNot)
        }
        _ => false,
    }
}

/// Recognize a sign-mask select on `x`, returning `(x, complemented)`:
///   `x < 0 ? -1 : 0` / `x >= 0 ? 0 : -1` → `(x, false)` — plain sign mask.
///   `x < 0 ? 0 : -1` / `x >= 0 ? -1 : 0` → `(x, true)`  — inverted sign mask.
/// Whether a `(cond) ? a : b` select lowers to a branchless sequence that emits NO compare —
/// a sign-mask (`srawi`/`srwi`) or a consecutive-constant sign select (`(x REL 0) ? c1 : c2`,
/// c1/c2 adjacent). The guard-sequence emitter uses this to know a folded tail won't emit a
/// redundant compare that would conflict with an earlier guard's compare on the same operand.
/// Conservative: relations whose select is NOT one of these (==0 / !=0 / <=0 / variable compares)
/// return false, so the caller keeps deferring — and those tails defer in evaluate_tail anyway.
pub(crate) fn select_folds_branchless(
    condition: &Expression,
    when_true: &Expression,
    when_false: &Expression,
) -> bool {
    sign_mask_select(condition, when_true, when_false).is_some()
        || sign_consecutive_select(condition, when_true, when_false).is_some()
        || zero_equal_consecutive(condition, when_true, when_false).is_some()
}

pub(crate) fn sign_mask_select<'e>(
    condition: &'e Expression,
    when_true: &'e Expression,
    when_false: &'e Expression,
) -> Option<(&'e Expression, bool)> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !is_zero_literal(right) {
        return None;
    }
    // Normalize the arms to (negative-case value, nonnegative-case value).
    let (negative_arm, nonnegative_arm) = match operator {
        BinaryOperator::Less => (when_true, when_false), // x < 0 ? a : b
        BinaryOperator::GreaterEqual => (when_false, when_true), // x >= 0 ? b : a
        _ => return None,
    };
    if constant_value(negative_arm) == Some(-1) && is_zero_literal(nonnegative_arm) {
        Some((left.as_ref(), false)) // -1 when negative, 0 otherwise
    } else if is_zero_literal(negative_arm) && constant_value(nonnegative_arm) == Some(-1) {
        Some((left.as_ref(), true)) // 0 when negative, -1 otherwise
    } else {
        None
    }
}

/// A recognized sign-compare select with consecutive non-zero constant arms.
/// `value` is the compared operand; `arithmetic` picks `srawi` (`-1/0`) vs `srwi`
/// (`0/1`); `offset` is the trailing `addi`. When `positive` is set the truth is
/// `x > 0`, needing a `neg; andc` preamble to form the mask base from `x`.
/// The preamble that places, into the scratch, a value whose SIGN BIT is set exactly when the
/// relation holds; `srawi`/`srwi` then broadcasts/extracts it. `None` uses the value's own sign
/// bit directly (`< 0` / `>= 0`).
#[derive(Clone, Copy)]
pub(crate) enum MaskPreamble {
    None,
    /// `neg; andc` — bit 31 set iff `> 0`.
    Andc,
    /// `neg; or` — bit 31 set iff `!= 0`.
    Or,
    /// `neg; orc` — bit 31 set iff `<= 0`.
    Orc,
}

pub(crate) struct SignConsecutive<'e> {
    pub(crate) value: &'e Expression,
    pub(crate) preamble: MaskPreamble,
    pub(crate) arithmetic: bool,
    pub(crate) offset: i16,
}

/// Recognize a sign-compare select with consecutive non-zero constant arms —
/// `(x < 0)`, `(x >= 0)`, or `(x > 0)` `? c1 : c2` with `|c1-c2| == 1`. The
/// shifted sign bit (`srawi`/`srwi x,31`, optionally after `neg; andc` for the
/// `> 0` case) plus an offset reproduces the two constants.
pub(crate) fn sign_consecutive_select<'e>(
    condition: &'e Expression,
    when_true: &Expression,
    when_false: &Expression,
) -> Option<SignConsecutive<'e>> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !is_zero_literal(right) {
        return None;
    }
    let (c1, c2) = (constant_value(when_true)?, constant_value(when_false)?);
    if c1 == 0 || c2 == 0 || (c1 - c2).abs() != 1 {
        return None;
    }
    let value = left.as_ref();
    match operator {
        // x < 0 ? c1 : c2 — the value's own sign bit; -1/0 (srawi) when the negative
        // arm c1 is the lower constant, else 0/1 (srwi). Both orders match mwcc.
        BinaryOperator::Less => Some(SignConsecutive {
            value,
            preamble: MaskPreamble::None,
            arithmetic: c1 < c2,
            offset: i16::try_from(c2).ok()?,
        }),
        // x >= 0 ? c1 : c2 — only the c1<c2 order is this clean `srwi d,x,31; addi c1`
        // form (the negative arm c2 is the higher constant). The reverse order uses
        // an extra `xori`, so defer it rather than emit the two-instruction shape.
        BinaryOperator::GreaterEqual if c1 < c2 => Some(SignConsecutive {
            value,
            preamble: MaskPreamble::None,
            arithmetic: false,
            offset: i16::try_from(c1).ok()?,
        }),
        // x > 0 ? c1 : c2 — `neg r0,x; andc r0,r0,x` sets bit 31 iff x > 0, then the
        // same srawi/srwi + addi c2. Both arm orders match mwcc.
        BinaryOperator::Greater => Some(SignConsecutive {
            value,
            preamble: MaskPreamble::Andc,
            arithmetic: c1 < c2,
            offset: i16::try_from(c2).ok()?,
        }),
        // x != 0 ? c1 : c2 — `neg r0,x; or r0,r0,x` sets bit 31 iff x != 0. Both orders match.
        BinaryOperator::NotEqual => Some(SignConsecutive {
            value,
            preamble: MaskPreamble::Or,
            arithmetic: c1 < c2,
            offset: i16::try_from(c2).ok()?,
        }),
        // x <= 0 ? c1 : c2 — `neg r0,x; orc r0,x,r0` sets bit 31 iff x <= 0. Only the c1<c2
        // (srawi) order matches this shape; the reverse uses a different cntlzw idiom, so defer.
        BinaryOperator::LessEqual if c1 < c2 => Some(SignConsecutive {
            value,
            preamble: MaskPreamble::Orc,
            arithmetic: true,
            offset: i16::try_from(c2).ok()?,
        }),
        _ => None,
    }
}

/// `(x == 0) ? c1 : c2` with consecutive non-zero constants — the cntlzw 0/1-flag idiom (NOT a
/// sign-bit mask, which `==` cannot use). Returns `(x, c1, c2)`.
pub(crate) fn zero_equal_consecutive<'e>(
    condition: &'e Expression,
    when_true: &Expression,
    when_false: &Expression,
) -> Option<(&'e Expression, i16, i16)> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !is_zero_literal(right) {
        return None;
    }
    let (c1, c2) = (constant_value(when_true)?, constant_value(when_false)?);
    if c1 == 0 || c2 == 0 || (c1 - c2).abs() != 1 {
        return None;
    }
    Some((
        left.as_ref(),
        i16::try_from(c1).ok()?,
        i16::try_from(c2).ok()?,
    ))
}
