//! Physical entry schedule for a saved receiver with mixed numeric conversions.
//!
//! Build 163 overlaps linkage setup with an integer-to-float image, establishes
//! the saved receiver before loading the conversion coefficients, and delays the
//! saved integer reload until the early-return comparison has issued.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_mixed_conversion_entry(&mut self) {
        let Some(start) = mixed_conversion_entry(&self.output.instructions) else {
            return;
        };

        self.move_instruction_before(start + 4, start + 2);
        self.move_instruction_before(start + 18, start + 5);
        self.move_instruction_before(start + 19, start + 6);
        self.move_instruction_before(start + 13, start + 9);
        self.move_instruction_before(start + 11, start + 10);
        self.move_instruction_before(start + 14, start + 13);

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start + 9] else {
            unreachable!("mixed conversion entry lost its coefficient base")
        };
        *d = 4;
        for relative in [13, 15] {
            let Instruction::LoadFloatSingle { a, .. } =
                &mut self.output.instructions[start + relative]
            else {
                unreachable!("mixed conversion entry lost a coefficient load")
            };
            *a = 4;
        }
        let Instruction::LoadFloatDouble { d, .. } = &mut self.output.instructions[start + 12]
        else {
            unreachable!("mixed conversion entry lost its assembled image")
        };
        *d = 2;
        let Instruction::FloatSubtractSingle { a, .. } = &mut self.output.instructions[start + 14]
        else {
            unreachable!("mixed conversion entry lost its bias subtraction")
        };
        *a = 2;

        self.move_instruction_before(start + 21, start + 19);
        self.move_instruction_before(start + 21, start + 20);
        self.move_instruction_before(start + 22, start + 21);
        let Instruction::LoadWord { offset, .. } = &mut self.output.instructions[start + 19]
        else {
            unreachable!("mixed conversion entry lost its comparison image")
        };
        *offset = 20;
        let Instruction::LoadWord { offset, .. } = &mut self.output.instructions[start + 22]
        else {
            unreachable!("mixed conversion entry lost its retained integer image")
        };
        *offset = 28;

        let Some(final_call) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadByteZero { d: 3, .. },
                    Instruction::AddImmediate { d: 4, a: 0, .. },
                    Instruction::AddImmediate { d: 5, a: 0, .. },
                    Instruction::Or { a: 6, s: 31, b: 31 },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        self.output.instructions[final_call + 3] = Instruction::AddImmediate {
            d: 6,
            a: 31,
            immediate: 0,
        };
        self.move_instruction_before(final_call + 3, final_call + 1);
    }
}

fn mixed_conversion_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(24).position(|window| {
        matches!(
            window,
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 },
                Instruction::StoreWord { s: 31, a: 1, offset: 44 },
                Instruction::XorImmediateShifted {
                    a: 0,
                    s: 4,
                    immediate: 0x8000,
                },
                Instruction::StoreWord { s: 0, a: 1, offset: 36 },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::LoadFloatDouble { d: 3, a: 0, offset: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 32 },
                Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 },
                Instruction::FloatSubtractSingle { d: 2, a: 0, b: 3 },
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 3,
                    offset: first_coefficient,
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 3,
                    offset: second_coefficient,
                },
                Instruction::FloatMultiplyAddSingle {
                    d: 0,
                    a: 2,
                    c: 1,
                    b: 0,
                },
                Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
                Instruction::StoreFloatDouble { s: 0, a: 1, offset: 16 },
                Instruction::LoadWord { d: 31, a: 1, offset: 20 },
                Instruction::StoreWord { s: 30, a: 1, offset: 40 },
                Instruction::Or { a: 30, s: 3, b: 3 },
                Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 },
                Instruction::LoadWord { d: 0, a: 1, offset: 28 },
                Instruction::CompareWordImmediate { a: 0, immediate: 1 },
                Instruction::BranchConditionalForward { .. },
            ] if *second_coefficient == first_coefficient + 4
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_frame_with_a_different_conversion_layout() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
        ];
        assert_eq!(mixed_conversion_entry(&instructions), None);
    }
}
