//! Equality tests against constants that do not fit a compare immediate.
//!
//! PowerPC has no 32-bit compare-immediate form. MWCC avoids materializing the
//! constant: it subtracts the upper half from the value with `addis`, then
//! compares the remaining low half with `cmplwi`.

use super::*;

impl Generator {
    pub(crate) fn try_emit_large_equality_compare(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
            || as_small_integer(right).is_some()
        {
            return Ok(None);
        }
        let Some(constant) = constant_value(right).and_then(integer_word_bits) else {
            return Ok(None);
        };
        let high = (constant >> 16) as u16;
        if high == 0 {
            return Ok(None);
        }

        // r0 cannot be the source of addis: an r0 base encodes literal zero.
        // Leaves and call results already have a suitable home. Evaluate other
        // computed/memory values directly into a non-r0 temporary rather than
        // first loading r0 and copying it.
        let source = match left {
            Expression::Variable(_)
            | Expression::Call { .. }
            | Expression::CallThrough { .. } => self.condition_operand_register(left)?,
            _ => {
                let register = self.lowest_free_general()?;
                self.evaluate_general(left, register)?;
                register
            }
        };
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: GENERAL_SCRATCH,
                a: source,
                immediate: (high as i16).wrapping_neg(),
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: GENERAL_SCRATCH,
                immediate: constant as u16,
            });
        Ok(Some(
            false_branch_bo_bi(operator).expect("equality has a false branch encoding"),
        ))
    }
}

fn integer_word_bits(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(|value| value as u32))
}

#[cfg(test)]
mod tests {
    use super::integer_word_bits;

    #[test]
    fn accepts_each_source_spelling_of_a_32_bit_word() {
        assert_eq!(integer_word_bits(0x1234_5678), Some(0x1234_5678));
        assert_eq!(integer_word_bits(u32::MAX as i64), Some(u32::MAX));
        assert_eq!(integer_word_bits(-1), Some(u32::MAX));
        assert_eq!(integer_word_bits(i32::MIN as i64), Some(0x8000_0000));
        assert_eq!(integer_word_bits(u32::MAX as i64 + 1), None);
        assert_eq!(integer_word_bits(i32::MIN as i64 - 1), None);
    }
}
