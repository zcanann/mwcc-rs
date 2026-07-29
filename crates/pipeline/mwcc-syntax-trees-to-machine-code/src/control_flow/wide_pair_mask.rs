//! Record-form tests of an explicitly lowered two-word mask value.
//!
//! Legacy optimizers retain the high and low halves of a zero-extended `u64`
//! through masking and comparison. The wide-local lowering exposes that value
//! graph with ordinary 32-bit expressions; this owner keeps the terminal
//! two-word test together so generic expression placement cannot collapse it
//! back into a scalar `rlwinm.` test.

use super::*;

struct WidePairMask<'a> {
    low: &'a str,
    high: &'a str,
    mask: i16,
}

impl Generator {
    pub(super) fn try_emit_wide_pair_mask_test(
        &mut self,
        condition: &Expression,
    ) -> Compilation<bool> {
        let Some(test) = recognize(condition) else {
            return Ok(false);
        };
        let Some(low) = self.lookup_general(test.low) else {
            return Ok(false);
        };
        let Some(high) = self.lookup_general(test.high) else {
            return Ok(false);
        };

        let low_masked = self.fresh_virtual_general_preferring(3);
        let high_masked = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        let low_compared = self.fresh_virtual_general_preferring(3);
        let high_compared = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        self.output
            .instructions
            .push(Instruction::load_immediate(low_masked, test.mask));
        self.output.instructions.push(Instruction::And {
            a: high_masked,
            s: high,
            b: high,
        });
        self.output.instructions.push(Instruction::And {
            a: low_masked,
            s: low,
            b: low_masked,
        });
        self.output.instructions.push(Instruction::Xor {
            a: low_compared,
            s: low_masked,
            b: high,
        });
        self.output.instructions.push(Instruction::Xor {
            a: high_compared,
            s: high_masked,
            b: high,
        });
        self.output.instructions.push(Instruction::OrRecord {
            a: GENERAL_SCRATCH,
            s: low_compared,
            b: high_compared,
        });
        Ok(true)
    }
}

fn recognize(condition: &Expression) -> Option<WidePairMask<'_>> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitXor,
        left: masked_low,
        right: low_zero,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitXor,
        left: masked_high,
        right: high_zero,
    } = right.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: low,
        right: mask,
    } = masked_low.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: high_left,
        right: high_right,
    } = masked_high.as_ref()
    else {
        return None;
    };
    let (
        Expression::Variable(low),
        Expression::IntegerLiteral(mask),
        Expression::Variable(high),
        Expression::Variable(high_right),
        Expression::Variable(low_zero),
        Expression::Variable(high_zero),
    ) = (
        low.as_ref(),
        mask.as_ref(),
        high_left.as_ref(),
        high_right.as_ref(),
        low_zero.as_ref(),
        high_zero.as_ref(),
    )
    else {
        return None;
    };
    if high != high_right || high != low_zero || high != high_zero {
        return None;
    }
    Some(WidePairMask {
        low,
        high,
        mask: i16::try_from(*mask).ok().filter(|mask| *mask > 0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_one_consistent_high_word() {
        let condition =
            crate::wide_local_scalarization::legacy_word_pair_mask_condition_for_test(
                "low", "high", 0x80,
            );
        let test = recognize(&condition).expect("the lowered pair graph should be recognized");
        assert_eq!((test.low, test.high, test.mask), ("low", "high", 0x80));
    }
}
