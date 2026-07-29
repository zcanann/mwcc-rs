//! Record-form tests of an explicitly lowered two-word mask value.
//!
//! Legacy optimizers retain the high and low halves of a zero-extended `u64`
//! through masking and comparison. The wide-local lowering exposes that value
//! graph with ordinary 32-bit expressions; this owner keeps the terminal
//! two-word test together so generic expression placement cannot collapse it
//! back into a scalar `rlwinm.` test.

use super::*;
use mwcc_vreg::Reg;

struct WidePairMask<'a> {
    low: &'a str,
    high: &'a str,
    zero: &'a str,
    mask: i16,
}

#[derive(Clone)]
struct RetainedHighMask {
    high: String,
    zero: String,
    high_source: u8,
    zero_source: u8,
    register: u8,
}

#[derive(Clone)]
struct RetainedZero {
    name: String,
    source: u8,
}

#[derive(Clone, Default)]
pub(crate) struct WidePairMaskCache {
    retained_high: Option<RetainedHighMask>,
    retained_zero: Option<RetainedZero>,
}

impl Generator {
    pub(crate) fn begin_wide_pair_mask_condition(
        &mut self,
        condition: &Expression,
    ) -> WidePairMaskCache {
        let previous = std::mem::take(&mut self.wide_pair_mask_cache);
        let retain_previous = recognize(condition).is_some_and(|test| {
            previous.retained_high.as_ref().is_some_and(|retained| {
                retained.high == test.high && retained.zero == test.zero
            })
        });
        if retain_previous {
            self.wide_pair_mask_cache = previous.clone();
        } else if !crate::condition_float_cache::expression_has_value_barrier(condition) {
            self.wide_pair_mask_cache.retained_zero = previous.retained_zero.clone();
        }
        previous
    }

    pub(crate) fn restore_wide_pair_mask_cache(&mut self, previous: WidePairMaskCache) {
        self.wide_pair_mask_cache = previous;
    }

    pub(crate) fn wide_pair_mask_false_edge_cache(&self) -> WidePairMaskCache {
        self.wide_pair_mask_cache.clone()
    }

    pub(crate) fn retained_wide_pair_zero_register(&self) -> Option<u8> {
        self.wide_pair_mask_cache
            .retained_zero
            .as_ref()
            .filter(|retained| self.lookup_general(&retained.name) == Some(retained.source))
            .map(|retained| retained.source)
    }

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
        let Some(zero) = self.lookup_general(test.zero) else {
            return Ok(false);
        };

        let mut displaced: Vec<_> = self
            .locations
            .values()
            .filter(|location| location.class == ValueClass::General)
            .map(|location| location.register)
            .filter(|register| *register != high && *register != low)
            .collect();
        displaced.extend(
            self.register_prefer
                .keys()
                .copied()
                .map(|register| Reg::Virtual(register).to_field())
                .filter(|register| *register != high && *register != low),
        );
        displaced.sort_unstable();
        displaced.dedup();
        for register in displaced {
            self.avoid_virtual_general(register, &[30, 31]);
        }
        self.prefer_virtual_general(high, 31);
        self.prefer_virtual_general(low, 30);
        let separate_zero = test.zero != test.high;
        if separate_zero {
            self.prefer_virtual_general(zero, 6);
        }
        let retained_high =
            retained_high_register(&self.wide_pair_mask_cache, &test, high, zero);
        let low_masked = self
            .fresh_virtual_general_preferring(if separate_zero { GENERAL_SCRATCH } else { 3 });
        let high_masked = retained_high.unwrap_or_else(|| {
            self.fresh_virtual_general_preferring(if separate_zero {
                5
            } else {
                GENERAL_SCRATCH
            })
        });
        let low_compared =
            self.fresh_virtual_general_preferring(if separate_zero { 4 } else { 3 });
        let high_compared = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        self.output
            .instructions
            .push(Instruction::load_immediate(low_masked, test.mask));
        let low_and = Instruction::And {
            a: low_masked,
            s: low,
            b: low_masked,
        };
        let high_and = Instruction::And {
            a: high_masked,
            s: high,
            b: zero,
        };
        if retained_high.is_some() {
            self.output.instructions.push(low_and);
        } else {
            if separate_zero {
                self.output.instructions.extend([low_and, high_and]);
            } else {
                self.output.instructions.extend([high_and, low_and]);
            }
            self.wide_pair_mask_cache.retained_high = Some(RetainedHighMask {
                high: test.high.into(),
                zero: test.zero.into(),
                high_source: high,
                zero_source: zero,
                register: high_masked,
            });
        }
        self.wide_pair_mask_cache.retained_zero = Some(RetainedZero {
            name: test.zero.into(),
            source: zero,
        });
        self.output.instructions.push(Instruction::Xor {
            a: low_compared,
            s: low_masked,
            b: zero,
        });
        self.output.instructions.push(Instruction::Xor {
            a: high_compared,
            s: high_masked,
            b: zero,
        });
        self.output.instructions.push(Instruction::OrRecord {
            a: GENERAL_SCRATCH,
            s: low_compared,
            b: high_compared,
        });
        Ok(true)
    }
}

fn retained_high_register(
    cache: &WidePairMaskCache,
    test: &WidePairMask<'_>,
    high_source: u8,
    zero_source: u8,
) -> Option<u8> {
    cache.retained_high.as_ref().and_then(|retained| {
        (retained.high == test.high
            && retained.zero == test.zero
            && retained.high_source == high_source
            && retained.zero_source == zero_source)
            .then_some(retained.register)
    })
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
    if high_right != low_zero || high_right != high_zero {
        return None;
    }
    Some(WidePairMask {
        low,
        high,
        zero: high_right,
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
        assert_eq!(
            (test.low, test.high, test.zero, test.mask),
            ("low", "high", "high", 0x80)
        );
    }

    #[test]
    fn retained_high_masks_require_the_same_value_homes() {
        let condition =
            crate::wide_local_scalarization::legacy_word_pair_mask_condition_for_test(
                "low", "high", 0x40,
            );
        let test = recognize(&condition).expect("the lowered pair graph should be recognized");
        let cache = WidePairMaskCache {
            retained_high: Some(RetainedHighMask {
                high: "high".into(),
                zero: "high".into(),
                high_source: 31,
                zero_source: 6,
                register: 5,
            }),
            retained_zero: Some(RetainedZero {
                name: "high".into(),
                source: 6,
            }),
        };
        assert_eq!(retained_high_register(&cache, &test, 31, 6), Some(5));
        assert_eq!(retained_high_register(&cache, &test, 30, 6), None);
    }
}
