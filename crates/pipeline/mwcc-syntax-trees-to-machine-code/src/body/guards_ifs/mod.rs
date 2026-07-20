//! If-statement and guard codegen. Split from the former single guards_ifs.rs
//! (fire 614) into cohesive submodules; behavior-identical.

mod call_return_chain;
mod early_return;
mod guard_block;
mod guard_sequence;
mod guarded_access;
mod leading;
mod legacy_tracked_return;
mod live_across;
mod trailing_if;

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Whether a comparison's operands are both signed — the case in which
    /// `emit_condition_test` emits a plain `cmpw`/`cmpwi` with no unsigned
    /// equality fold, so another branch can consume the same CR0 value.
    fn comparison_operands_signed(&self, condition: &Expression) -> bool {
        matches!(condition, Expression::Binary { left, right, .. }
            if self.signedness_of(left).unwrap_or(false) && self.signedness_of(right).unwrap_or(false))
    }
}

/// Whether two conditions are relational comparisons of the SAME operand against the
/// SAME value (`c > 0` and `c < 0`, both `cmpwi r3,0`). mwcc emits ONE compare and reads
/// its condition register from both branches; our per-branch re-compare would emit a
/// redundant second `cmpwi`, so the else-if chain defers when this holds.
pub(crate) fn shares_condition_register(a: &Expression, b: &Expression) -> bool {
    let relational = |operator: &BinaryOperator| {
        matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterEqual
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
        )
    };
    match (a, b) {
        (
            Expression::Binary {
                operator: operator_a,
                left: left_a,
                right: right_a,
            },
            Expression::Binary {
                operator: operator_b,
                left: left_b,
                right: right_b,
            },
        ) if relational(operator_a) && relational(operator_b) => {
            same_operand(left_a, left_b) && same_operand(right_a, right_b)
        }
        _ => false,
    }
}
