//! Pure predicates and shape queries over expressions — no `Generator` state.

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type, UnaryOperator};

/// Whether an expression contains a call anywhere.
pub(crate) fn expression_has_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. } => true,
        Expression::Binary { left, right, .. } => expression_has_call(left) || expression_has_call(right),
        Expression::Unary { operand, .. } => expression_has_call(operand),
        Expression::Conditional { condition, when_true, when_false } => {
            expression_has_call(condition) || expression_has_call(when_true) || expression_has_call(when_false)
        }
        Expression::Cast { operand, .. } => expression_has_call(operand),
        Expression::Dereference { pointer } => expression_has_call(pointer),
        Expression::Index { base, index } => expression_has_call(base) || expression_has_call(index),
        _ => false,
    }
}

/// Whether a function makes a call (and so needs the non-leaf prologue).
pub(crate) fn statement_has_call(statement: &Statement) -> bool {
    match statement {
        Statement::Store { target, value } => expression_has_call(target) || expression_has_call(value),
        Statement::Assign { value, .. } => expression_has_call(value),
        Statement::Expression(expression) => expression_has_call(expression),
        Statement::Switch { scrutinee, arms, default } => {
            expression_has_call(scrutinee)
                || arms.iter().any(|arm| expression_has_call(&arm.result))
                || default.as_ref().is_some_and(expression_has_call)
        }
        Statement::If { condition, then_body, else_body } => {
            expression_has_call(condition) || block_has_call(then_body) || block_has_call(else_body)
        }
    }
}

pub(crate) fn block_has_call(statements: &[Statement]) -> bool {
    statements.iter().any(statement_has_call)
}

pub(crate) fn function_makes_call(function: &Function) -> bool {
    function.statements.iter().any(statement_has_call)
        || function.return_expression.as_ref().is_some_and(expression_has_call)
        || function.locals.iter().any(|local| local.initializer.as_ref().is_some_and(expression_has_call))
        || function.guards.iter().any(|guard| expression_has_call(&guard.condition) || expression_has_call(&guard.value))
}

pub(crate) fn is_complex(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary { .. } | Expression::Unary { .. } | Expression::Conditional { .. } | Expression::Cast { .. }
    )
}

/// The Sethi-Ullman register need of an expression: the number of registers
/// needed to evaluate it without spilling. mwcc evaluates the operand with the
/// *higher* need first — the heavier subtree, independent of source order — which
/// is the key to matching its instruction order on asymmetric arithmetic trees
/// (`((b+c)*(d+e)) + a` and `a + ((b+c)*(d+e))` compile identically because the
/// heavy product is always done first). A leaf needs one register; a binary node
/// needs `n+1` when its two operands tie at `n` (the second result must survive
/// while the first is computed), otherwise the larger of the two — the heavier
/// side absorbs the lighter for free. Loads/calls are approximated as leaves;
/// refine when the placement restructure consumes this.
///
pub(crate) fn register_need(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary { left, right, .. } => {
            let left_need = register_need(left);
            let right_need = register_need(right);
            if left_need == right_need {
                left_need + 1
            } else {
                left_need.max(right_need)
            }
        }
        Expression::Unary { operand, .. } => register_need(operand),
        Expression::Cast { operand, .. } => register_need(operand),
        Expression::Conditional { when_true, when_false, .. } => {
            register_need(when_true).max(register_need(when_false)).max(1)
        }
        _ => 1,
    }
}

/// If `expression` is `*pointer`, the pointer sub-expression.
pub(crate) fn as_dereference(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Dereference { pointer } => Some(pointer),
        _ => None,
    }
}

/// If `expression` is `base->field`, its base, byte offset, and member type.
pub(crate) fn as_member(expression: &Expression) -> Option<(&Expression, u16, mwcc_syntax_trees::Type)> {
    match expression {
        Expression::Member { base, offset, member_type } => Some((base, *offset, *member_type)),
        _ => None,
    }
}

pub(crate) fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

/// The integer value if `expression` is a literal or a negated literal.
pub(crate) fn constant_value(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value),
        // Fold `-c` and `~c` of a constant operand, so e.g. `x & ~7` becomes a
        // mask immediate rather than falling into a broken two-operand path.
        Expression::Unary { operator: UnaryOperator::Negate, operand } => constant_value(operand).map(|value| value.wrapping_neg()),
        Expression::Unary { operator: UnaryOperator::BitNot, operand } => constant_value(operand).map(|value| !value),
        Expression::Binary { operator, left, right } => {
            use BinaryOperator::*;
            // `x - x` and `x ^ x` are 0 for any side-effect-free operand, even a
            // non-constant one (mwcc folds them without touching memory).
            if matches!(operator, Subtract | BitXor) && same_operand(left, right) {
                return Some(0);
            }
            // Otherwise fold arithmetic of two compile-time constants (`2 + 3`,
            // `FLAG_A | FLAG_B`, `1 << 3`), matching mwcc's `li`/`lis;ori`. The
            // result is truncated to 32 bits (C `int` arithmetic) so e.g. `1 << 31`
            // is the negative `0x80000000`, materialized by a single `lis`.
            let (l, r) = (constant_value(left)?, constant_value(right)?);
            let folded = match operator {
                Add => l.wrapping_add(r),
                Subtract => l.wrapping_sub(r),
                Multiply => l.wrapping_mul(r),
                BitAnd => l & r,
                BitOr => l | r,
                BitXor => l ^ r,
                ShiftLeft if (0..32).contains(&r) => l.wrapping_shl(r as u32),
                ShiftRight if (0..32).contains(&r) => l >> r,
                _ => return None,
            };
            Some(folded as i32 as i64)
        }
        _ => None,
    }
}

/// Whether two expressions are the SAME side-effect-free value — identical
/// variable, dereference, member, or subscript (recursively). Calls and other
/// effectful nodes never match, so `x - x`/`x == x` style identities are only
/// folded when re-evaluating `x` would be observably identical.
pub(crate) fn same_operand(a: &Expression, b: &Expression) -> bool {
    match (a, b) {
        (Expression::IntegerLiteral(x), Expression::IntegerLiteral(y)) => x == y,
        (Expression::Variable(x), Expression::Variable(y)) => x == y,
        (Expression::Dereference { pointer: pa }, Expression::Dereference { pointer: pb }) => same_operand(pa, pb),
        (Expression::Member { base: ba, offset: oa, .. }, Expression::Member { base: bb, offset: ob, .. }) => oa == ob && same_operand(ba, bb),
        (Expression::Index { base: ba, index: ia }, Expression::Index { base: bb, index: ib }) => same_operand(ba, bb) && same_operand(ia, ib),
        _ => false,
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

/// Decompose `x & mask` where `x` is a leaf variable and `mask` an integer
/// literal. Returns `(x, mask)` with the mask narrowed to 32 bits.
pub(crate) fn as_masked_leaf(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expression else { return None };
    leaf_name(left)?;
    match **right {
        Expression::IntegerLiteral(mask) => Some((left, mask as u32)),
        _ => None,
    }
}

/// Decompose `load & mask` where `load` is a memory load (dereference, member,
/// or index) and `mask` an integer literal. Returns `(load, mask)`.
pub(crate) fn as_masked_load(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expression else { return None };
    if !matches!(left.as_ref(), Expression::Dereference { .. } | Expression::Member { .. } | Expression::Index { .. }) {
        return None;
    }
    match **right {
        Expression::IntegerLiteral(mask) => Some((left, mask as u32)),
        _ => None,
    }
}

/// If `mask` is a single contiguous run of set bits, return its PowerPC
/// `[begin, end]` bit span (bit 0 = the most significant bit). Non-contiguous
/// (or wrapping) masks return `None`.
pub(crate) fn mask_to_run(mask: u32) -> Option<(u8, u8)> {
    if mask == 0 {
        return None;
    }
    let begin = mask.leading_zeros() as u8;
    let end = 31 - mask.trailing_zeros() as u8;
    let expected = run_mask(begin, end);
    (expected == mask).then_some((begin, end))
}

/// The 32-bit mask whose set bits are the contiguous run `[begin, end]`
/// (bit 0 = the most significant bit).
pub(crate) fn run_mask(begin: u8, end: u8) -> u32 {
    (0xFFFF_FFFFu32 >> begin) & (0xFFFF_FFFFu32 << (31 - end))
}

/// How one operand of a bitfield merge produces its contiguous masked region.
pub(crate) enum FieldSource {
    ShiftLeft(u8),
    ShiftRight(u8),
    Mask,
}

/// Decompose an expression into a contiguous bit field of a leaf variable: a
/// constant shift (`x << n` / `x >> n`) or a mask (`x & m`). Returns the
/// variable, how the field is produced, and its PowerPC `[begin, end]` span.
pub(crate) fn as_field(expression: &Expression) -> Option<(&Expression, FieldSource, u8, u8)> {
    if let Some((value, is_left, shift)) = as_constant_shift(expression) {
        return Some(if is_left {
            (value, FieldSource::ShiftLeft(shift), 0, 31 - shift)
        } else {
            (value, FieldSource::ShiftRight(shift), shift, 31)
        });
    }
    if let Some((value, mask)) = as_masked_leaf(expression) {
        let (begin, end) = mask_to_run(mask)?;
        return Some((value, FieldSource::Mask, begin, end));
    }
    None
}

/// A nonzero integer literal that fits a signed 16-bit immediate.
pub(crate) fn as_small_integer(expression: &Expression) -> Option<i16> {
    match expression {
        Expression::IntegerLiteral(value) if *value != 0 => i16::try_from(*value).ok(),
        _ => None,
    }
}

/// Decompose a constant shift of a leaf variable: `x << c` or `x >> c` with
/// `c` in `1..=31`. Returns `(x, is_left_shift, c)`. Used to recognize the
/// rotate idiom `(x << c) | (x >> (32-c))`.
pub(crate) fn as_constant_shift(expression: &Expression) -> Option<(&Expression, bool, u8)> {
    let Expression::Binary { operator, left, right } = expression else { return None };
    let is_left = match operator {
        BinaryOperator::ShiftLeft => true,
        BinaryOperator::ShiftRight => false,
        _ => return None,
    };
    leaf_name(left)?;
    match **right {
        Expression::IntegerLiteral(amount) if (1..=31).contains(&amount) => Some((left, is_left, amount as u8)),
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

/// A 32-bit mask representable by a single `rlwinm rA,rS,0,MB,ME` — a contiguous
/// run of set bits, possibly wrapping around bit 31->0 (then `begin > end`, e.g.
/// `x & ~16` clears one bit via `rlwinm 0,28,26`). Returns the `(begin, end)`
/// mask-bit pair, or `None` for an all-clear mask or one with two or more runs.
pub(crate) fn rlwinm_mask(value: i64) -> Option<(u8, u8)> {
    if value as u32 == 0 {
        return None;
    }
    if let Some(run) = contiguous_mask(value) {
        return Some(run);
    }
    // A wrapping run of set bits: its complement is a non-wrapping run. If the
    // cleared bits are the run `[begin, end]`, the set bits run from `end+1`
    // wrapping to `begin-1`.
    let (begin, end) = contiguous_mask(!(value as u32) as i64)?;
    Some(((end + 1) & 31, (begin + 31) & 31))
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
            // Both operands complex: the left side computes into a fresh virtual
            // the allocator places and the right into the scratch, so this fits
            // even when the result itself lands in the scratch (the temporary is
            // no longer a physical register we must find).
            (true, true) => fits_single_scratch(left, false) && fits_single_scratch(right, true),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> Expression {
        Expression::Variable(name.to_string())
    }
    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary { operator, left: Box::new(left), right: Box::new(right) }
    }
    fn add(left: Expression, right: Expression) -> Expression {
        binary(BinaryOperator::Add, left, right)
    }
    fn mul(left: Expression, right: Expression) -> Expression {
        binary(BinaryOperator::Multiply, left, right)
    }

    #[test]
    fn a_leaf_needs_one_register() {
        assert_eq!(register_need(&var("a")), 1);
        assert_eq!(register_need(&Expression::IntegerLiteral(5)), 1);
    }

    #[test]
    fn two_leaves_under_a_binary_need_two() {
        // a + b: equal leaves (1,1) -> 2.
        assert_eq!(register_need(&add(var("a"), var("b"))), 2);
    }

    #[test]
    fn balanced_trees_grow_by_one_per_level() {
        // (a+b)*(c+d): both sides 2, equal -> 3.
        let left = add(var("a"), var("b"));
        let right = add(var("c"), var("d"));
        assert_eq!(register_need(&mul(left, right)), 3);
    }

    #[test]
    fn a_heavier_subtree_absorbs_a_lighter_one_for_free() {
        // a + ((b+c)*(d+e)): leaf (1) vs heavy product (3) -> max = 3, not 4.
        let heavy = mul(add(var("b"), var("c")), add(var("d"), var("e")));
        assert_eq!(register_need(&heavy), 3);
        assert_eq!(register_need(&add(var("a"), heavy.clone())), 3);
        // And the need is the same whichever side the heavy subtree is on — the
        // property that makes mwcc's order independent of source order.
        assert_eq!(register_need(&add(heavy, var("a"))), 3);
    }

    #[test]
    fn the_heavier_operand_is_identifiable_by_comparing_needs() {
        // c + a*b: c (1) lighter than a*b (2); the multiply is evaluated first.
        let product = mul(var("a"), var("b"));
        assert!(register_need(&product) > register_need(&var("c")));
    }
}
