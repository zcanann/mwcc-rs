//! Reuse a zero across a mixed-width member reset sequence.
//!
//! Inline-composed state transitions often interleave integer zero stores with
//! a float copy and a bit-field clear. The bit-field path already needs zero in
//! r3; MWCC materializes it at the first store and retains it across the whole
//! reset instead of recreating zero in r0 for each scalar width.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_mixed_member_zero_reset(&mut self) {
        let mut start = 0;
        while start + 17 <= self.output.instructions.len() {
            if !is_unscheduled_reset(&self.output.instructions[start..start + 17]) {
                start += 1;
                continue;
            }

            match &mut self.output.instructions[start] {
                Instruction::AddImmediate { d, .. } => *d = 3,
                _ => unreachable!(),
            }
            for index in [start + 1, start + 5, start + 7, start + 13] {
                match &mut self.output.instructions[index] {
                    Instruction::StoreWord { s, .. } | Instruction::StoreByte { s, .. } => *s = 3,
                    _ => unreachable!(),
                }
            }

            // Later zero materializations are now dead. Remove them in reverse
            // source order so all original indices above stay meaningful.
            for index in [start + 12, start + 9, start + 6, start + 4] {
                self.remove_structured_condition_instruction(index);
            }
            start += 13;
        }
    }
}

fn is_zero(instruction: &Instruction, register: u8) -> bool {
    matches!(instruction, Instruction::AddImmediate { d, a: 0, immediate: 0 } if *d == register)
}

fn is_unscheduled_reset(window: &[Instruction]) -> bool {
    matches!(window, [
        first_zero,
        Instruction::StoreWord { s: 0, a: first_base, .. },
        Instruction::LoadFloatSingle { a: float_load_base, .. },
        Instruction::StoreFloatSingle { a: float_store_base, .. },
        second_zero,
        Instruction::StoreByte { s: 0, a: second_base, .. },
        third_zero,
        Instruction::StoreByte { s: 0, a: third_base, .. },
        Instruction::LoadByteZero { d: 0, a: bit_base, offset: bit_offset },
        bit_zero,
        Instruction::RotateAndMaskInsert { a: 0, s: 3, .. },
        Instruction::StoreByte { s: 0, a: bit_store_base, offset: bit_store_offset },
        fourth_zero,
        Instruction::StoreWord { s: 0, a: fourth_base, .. },
        Instruction::LoadWord { d: 0, a: mask_base, offset: mask_offset },
        Instruction::AndContiguousMask { a: 0, s: 0, .. },
        Instruction::StoreWord { s: 0, a: mask_store_base, offset: mask_store_offset },
    ] if is_zero(first_zero, 0)
        && is_zero(second_zero, 0)
        && is_zero(third_zero, 0)
        && is_zero(bit_zero, 3)
        && is_zero(fourth_zero, 0)
        && first_base == float_load_base
        && float_load_base == float_store_base
        && float_store_base == second_base
        && second_base == third_base
        && third_base == bit_base
        && bit_base == bit_store_base
        && bit_store_base == fourth_base
        && fourth_base == mask_base
        && mask_base == mask_store_base
        && bit_offset == bit_store_offset
        && mask_offset == mask_store_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_mixed_width_reset_around_a_bit_clear() {
        let instructions = [
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 224 },
            Instruction::LoadFloatSingle { d: 0, a: 31, offset: 128 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 236 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreByte { s: 0, a: 31, offset: 6504 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreByte { s: 0, a: 31, offset: 6505 },
            Instruction::LoadByteZero { d: 0, a: 31, offset: 8743 },
            Instruction::load_immediate(3, 0),
            Instruction::RotateAndMaskInsert { a: 0, s: 3, shift: 7, begin: 24, end: 24 },
            Instruction::StoreByte { s: 0, a: 31, offset: 8743 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 2188 },
            Instruction::LoadWord { d: 0, a: 31, offset: 2080 },
            Instruction::AndContiguousMask { a: 0, s: 0, begin: 28, end: 26 },
            Instruction::StoreWord { s: 0, a: 31, offset: 2080 },
        ];
        assert!(is_unscheduled_reset(&instructions));
    }
}
