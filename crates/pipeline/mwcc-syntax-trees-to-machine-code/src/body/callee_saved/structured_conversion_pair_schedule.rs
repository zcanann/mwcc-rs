//! Shared magic-bias setup for adjacent signed-integer conversions.
//!
//! Two integer arguments converted for one call use the same `0x4330` high word
//! and signed bias. MWCC loads each once, keeps them live through the first
//! subtraction, and reuses them for the second conversion image.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_signed_conversion_pair(&mut self) {
        let Some(start) = signed_conversion_pair(&self.output.instructions) else {
            return;
        };
        if !schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start + 2,
            start + 10,
        ) || !schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start + 3,
            start + 11,
        ) {
            return;
        }

        let second_result = match self.output.instructions[start + 15] {
            Instruction::FloatSubtractSingle { d, .. } => d,
            _ => unreachable!("conversion pair was recognized"),
        };
        match &mut self.output.instructions[start + 3] {
            Instruction::LoadFloatDouble { d, .. } => *d = second_result,
            _ => unreachable!("conversion pair was recognized"),
        }
        match &mut self.output.instructions[start + 7] {
            Instruction::FloatSubtractSingle { b, .. } => *b = second_result,
            _ => unreachable!("conversion pair was recognized"),
        }

        for relative in [11, 10, 9] {
            self.remove_conversion_pair_instruction(start + relative);
        }
    }

    fn remove_conversion_pair_instruction(&mut self, index: usize) {
        self.output.instructions.remove(index);
        self.labels.removed_retargeting_to_next(index, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > index {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > index =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }
}

fn signed_conversion_pair(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(16).position(|window| {
        matches!(
            window,
            [
                Instruction::XorImmediateShifted {
                    a: first_value,
                    s: first_source,
                    immediate: 0x8000,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::AddImmediateShifted {
                    d: first_bias_base,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatDouble {
                    d: first_bias,
                    a: first_load_base,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: first_low,
                    a: 1,
                    offset: first_low_offset,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: first_high_offset,
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: first_image_offset,
                },
                Instruction::FloatSubtractSingle {
                    d: first_result,
                    a: 0,
                    b: first_subtract_bias,
                },
                Instruction::XorImmediateShifted {
                    a: second_value,
                    s: second_source,
                    immediate: 0x8000,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::AddImmediateShifted {
                    d: second_bias_base,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatDouble {
                    d: second_bias,
                    a: second_load_base,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: second_low,
                    a: 1,
                    offset: second_low_offset,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: second_high_offset,
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: second_image_offset,
                },
                Instruction::FloatSubtractSingle {
                    d: second_result,
                    a: 0,
                    b: second_subtract_bias,
                },
            ] if first_value == first_source
                && first_value == first_low
                && second_value == second_source
                && second_value == second_low
                && first_bias_base == first_load_base
                && second_bias_base == second_load_base
                && first_bias == first_result
                && first_bias == first_subtract_bias
                && second_bias == second_result
                && second_bias == second_subtract_bias
                && first_result != second_result
                && *first_low_offset == first_high_offset + 4
                && first_high_offset == first_image_offset
                && *second_low_offset == second_high_offset + 4
                && second_high_offset == second_image_offset
        )
    })
}

#[cfg(test)]
#[path = "structured_conversion_pair_schedule_tests.rs"]
mod tests;
