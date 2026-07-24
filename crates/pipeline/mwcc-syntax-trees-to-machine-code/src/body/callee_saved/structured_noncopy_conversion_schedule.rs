//! Conversion-lane scheduling for the structured non-copy rendering arm.
//!
//! Two adjacent unsigned-halfword conversions overlap in MWCC's schedule. The
//! first conversion uses `r1+24`, the second uses `r1+32`, and both high words
//! are initialized while `r0` still holds `0x4330`. Their integer result images
//! occupy the otherwise dead `r1+16` and `r1+40` lanes. Keeping this as one
//! complete transaction makes the lane liveness and the removed high-word
//! materialization explicit.

#[allow(unused_imports)]
use super::*;

const CONVERSION_SCHEDULE: [usize; 30] = [
    0, 3, 4, 7, 1, 5, 6, 28, 2, 8, 20, 9, 29, 10, 11, 12, 13, 14, 15, 17, 16, 19, 21, 22, 23, 24,
    25, 26, 27, 18,
];

impl Generator {
    pub(crate) fn finalize_structured_noncopy_conversion_lanes(&mut self) {
        let Some(start) = noncopy_conversion_region(&self.output.instructions) else {
            return;
        };
        if !has_conversion_relocations(self, start)
            || !has_reciprocal_tail(&self.output.instructions, start + 30)
        {
            return;
        }

        let mut current: Vec<usize> = (0..CONVERSION_SCHEDULE.len()).collect();
        for (destination, &original) in CONVERSION_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the non-copy conversion schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        self.remove_noncopy_conversion_instruction(start + 29);
        assign_conversion_registers_and_lanes(&mut self.output.instructions[start..start + 29]);
        for divide in [start + 30, start + 36] {
            let Instruction::FloatDivideSingle { a, .. } = &mut self.output.instructions[divide]
            else {
                unreachable!("the reciprocal tail was matched")
            };
            *a = 1;
        }
    }

    fn remove_noncopy_conversion_instruction(&mut self, index: usize) {
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

fn noncopy_conversion_region(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(30).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadHalfwordZero {
                    d: first_value,
                    a: source,
                    offset: 8,
                },
                Instruction::ShiftLeftImmediate {
                    a: first_scaled,
                    s: first_shift_source,
                    shift: 2,
                },
                Instruction::LoadFloatSingle {
                    d: first_member,
                    a: first_member_base,
                    offset: 28,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::AddImmediateShifted {
                    d: bias_base,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatDouble {
                    d: bias,
                    a: bias_load_base,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: first_low,
                    a: 1,
                    offset: 12,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 8,
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: 8,
                },
                Instruction::FloatSubtractSingle {
                    d: 0,
                    a: 0,
                    b: first_bias,
                },
                Instruction::FloatMultiplySingle {
                    d: 0,
                    a: 0,
                    c: first_product,
                },
                Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
                Instruction::StoreFloatDouble {
                    s: 0,
                    a: 1,
                    offset: 32,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 36,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: object,
                    offset: 6,
                },
                Instruction::LoadHalfwordZero {
                    d: second_value,
                    a: second_source,
                    offset: 10,
                },
                Instruction::ShiftLeftImmediate {
                    a: second_scaled,
                    s: second_shift_source,
                    shift: 2,
                },
                Instruction::LoadFloatSingle {
                    d: second_member,
                    a: second_member_base,
                    offset: 32,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::StoreWord {
                    s: second_low,
                    a: 1,
                    offset: 12,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 8,
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: 8,
                },
                Instruction::FloatSubtractSingle {
                    d: 0,
                    a: 0,
                    b: second_bias,
                },
                Instruction::FloatMultiplySingle {
                    d: 0,
                    a: 0,
                    c: second_product,
                },
                Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
                Instruction::StoreFloatDouble {
                    s: 0,
                    a: 1,
                    offset: 40,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 44,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: second_object,
                    offset: 14,
                },
                Instruction::AddImmediateShifted {
                    d: scale_base,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle {
                    d: _,
                    a: scale_load_base,
                    offset: 0,
                },
            ] if first_value == first_shift_source
                && first_scaled == first_low
                && source == first_member_base
                && bias_base == bias_load_base
                && bias == first_bias
                && first_member == first_product
                && second_value == second_shift_source
                && second_scaled == second_low
                && source == second_source
                && source == second_member_base
                && bias == second_bias
                && second_member == second_product
                && object == second_object
                && scale_base == scale_load_base
        )
    })
}

fn has_conversion_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 30).contains(&relocation.instruction_index))
        .map(|relocation| relocation.instruction_index - start)
        .collect::<Vec<_>>();
    relative == [4, 5, 28, 29]
        && schedule_relocations::same_target_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 4,
            start + 5,
        )
        && schedule_relocations::same_target_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 28,
            start + 29,
        )
}

fn has_reciprocal_tail(instructions: &[Instruction], start: usize) -> bool {
    matches!(
        instructions.get(start..start + 12),
        Some([
            Instruction::LoadFloatSingle {
                d: 0,
                a: first_base,
                offset: 28,
            },
            Instruction::FloatDivideSingle {
                d: 0,
                a: scale,
                b: 0,
            },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 48,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 52,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: first_object,
                offset: 28,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: second_base,
                offset: 32,
            },
            Instruction::FloatDivideSingle {
                d: 0,
                a: second_scale,
                b: 0,
            },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 56,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 60,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: second_object,
                offset: 30,
            },
        ]) if first_base == second_base
            && scale == second_scale
            && first_object == second_object
    )
}

fn assign_conversion_registers_and_lanes(instructions: &mut [Instruction]) {
    let [first_load, _high, bias_high, first_high_store, first_shift, bias_load, first_low_store, scale_high, first_member, first_double, second_high_store, first_subtract, scale_load, first_multiply, _first_convert, first_result_store, first_result_load, _first_result_member_store, second_load, second_member, second_shift, second_low_store, second_double, second_subtract, second_multiply, _second_convert, _second_result_stack_store, _second_result_load, _second_result_member_store] =
        instructions
    else {
        unreachable!("the complete non-copy conversion schedule was recognized")
    };

    set_halfword_load(first_load, 5);
    set_shifted_destination(bias_high, 4);
    set_word_store(first_high_store, 0, 24);
    set_shift_register(first_shift, 5);
    set_double_load(bias_load, 3, 4, 0);
    set_word_store(first_low_store, 5, 28);
    set_shifted_destination(scale_high, 3);
    set_single_load(first_member, 0, None);
    set_double_load(first_double, 1, 1, 24);
    set_word_store(second_high_store, 0, 32);
    set_float_subtract(first_subtract, 2, 1, 3);
    set_single_load(scale_load, 1, Some(3));
    set_float_multiply(first_multiply, 0, 2, 0);
    set_double_store_offset(first_result_store, 16);
    set_word_load_offset(first_result_load, 20);

    set_halfword_load(second_load, 0);
    set_single_load(second_member, 0, None);
    set_shift_register(second_shift, 0);
    set_word_store(second_low_store, 0, 36);
    set_double_load(second_double, 2, 1, 32);
    set_float_subtract(second_subtract, 2, 2, 3);
    set_float_multiply(second_multiply, 0, 2, 0);
}

fn set_halfword_load(instruction: &mut Instruction, destination: u8) {
    let Instruction::LoadHalfwordZero { d, .. } = instruction else {
        unreachable!()
    };
    *d = destination;
}

fn set_shifted_destination(instruction: &mut Instruction, destination: u8) {
    let Instruction::AddImmediateShifted { d, .. } = instruction else {
        unreachable!()
    };
    *d = destination;
}

fn set_word_store(instruction: &mut Instruction, source: u8, new_offset: i16) {
    let Instruction::StoreWord { s, offset, .. } = instruction else {
        unreachable!()
    };
    *s = source;
    *offset = new_offset;
}

fn set_shift_register(instruction: &mut Instruction, register: u8) {
    let Instruction::ShiftLeftImmediate { a, s, .. } = instruction else {
        unreachable!()
    };
    *a = register;
    *s = register;
}

fn set_double_load(instruction: &mut Instruction, destination: u8, base: u8, new_offset: i16) {
    let Instruction::LoadFloatDouble { d, a, offset } = instruction else {
        unreachable!()
    };
    *d = destination;
    *a = base;
    *offset = new_offset;
}

fn set_single_load(instruction: &mut Instruction, destination: u8, base: Option<u8>) {
    let Instruction::LoadFloatSingle { d, a, .. } = instruction else {
        unreachable!()
    };
    *d = destination;
    if let Some(base) = base {
        *a = base;
    }
}

fn set_float_subtract(instruction: &mut Instruction, destination: u8, left: u8, right: u8) {
    let Instruction::FloatSubtractSingle { d, a, b } = instruction else {
        unreachable!()
    };
    *d = destination;
    *a = left;
    *b = right;
}

fn set_float_multiply(instruction: &mut Instruction, destination: u8, left: u8, right: u8) {
    let Instruction::FloatMultiplySingle { d, a, c } = instruction else {
        unreachable!()
    };
    *d = destination;
    *a = left;
    *c = right;
}

fn set_double_store_offset(instruction: &mut Instruction, new_offset: i16) {
    let Instruction::StoreFloatDouble { offset, .. } = instruction else {
        unreachable!()
    };
    *offset = new_offset;
}

fn set_word_load_offset(instruction: &mut Instruction, new_offset: i16) {
    let Instruction::LoadWord { offset, .. } = instruction else {
        unreachable!()
    };
    *offset = new_offset;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_two_serial_unsigned_conversion_lanes() {
        let instructions = vec![
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 26,
                offset: 8,
            },
            Instruction::ShiftLeftImmediate {
                a: 9,
                s: 0,
                shift: 2,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 26,
                offset: 28,
            },
            Instruction::load_immediate_shifted(0, 0x4330),
            Instruction::load_immediate_shifted(10, 0),
            Instruction::LoadFloatDouble {
                d: 2,
                a: 10,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 9,
                a: 1,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 8,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 31,
                offset: 6,
            },
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 26,
                offset: 10,
            },
            Instruction::ShiftLeftImmediate {
                a: 9,
                s: 0,
                shift: 2,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 26,
                offset: 32,
            },
            Instruction::load_immediate_shifted(0, 0x4330),
            Instruction::StoreWord {
                s: 9,
                a: 1,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 8,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 40,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 31,
                offset: 14,
            },
            Instruction::load_immediate_shifted(9, 0),
            Instruction::LoadFloatSingle {
                d: 3,
                a: 9,
                offset: 0,
            },
        ];

        assert_eq!(noncopy_conversion_region(&instructions), Some(0));
    }
}
