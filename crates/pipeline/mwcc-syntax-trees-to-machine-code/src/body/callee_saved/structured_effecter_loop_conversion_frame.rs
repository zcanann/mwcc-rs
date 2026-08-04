//! Scratch-image layout for the dense effecter loop's scaled output.
//!
//! The preheader conversion and the per-iteration unsigned conversion have
//! disjoint lifetimes. Build 163 reuses the upper image at 32/36 for the loop's
//! input, then writes the integer result through 24/28. Keeping this ownership
//! separate from preheader issue scheduling makes the frame reuse explicit.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn normalize_structured_effecter_loop_conversion_frame(&mut self) -> bool {
        let Some(start) = self
            .output
            .instructions
            .windows(9)
            .position(conversion_packet)
        else {
            return false;
        };
        assign_frame(&mut self.output.instructions[start..start + 9]);
        true
    }
}

fn conversion_packet(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::LoadHalfwordZero { d: 3, a: 0, offset: 0 },
            Instruction::StoreWord { s: 3, a: 1, offset: 12 },
            Instruction::StoreWord { s: 28, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 26 }
                | Instruction::FloatSubtractDouble { d: 0, a: 0, b: 26 },
            Instruction::FloatMultiplySingle { d: 0, a: 20, c: 0 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        ]
    )
}

fn assign_frame(window: &mut [Instruction]) {
    window[0] = Instruction::LoadHalfwordZero { d: 0, a: 0, offset: 0 };
    window[1] = Instruction::StoreWord { s: 0, a: 1, offset: 36 };
    window[2] = Instruction::StoreWord { s: 28, a: 1, offset: 32 };
    window[3] = Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 };
    window[7] = Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 };
    window[8] = Instruction::LoadWord { d: 0, a: 1, offset: 28 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_preheader_input_and_result_scratch_images() {
        let mut instructions = vec![
            Instruction::LoadHalfwordZero { d: 3, a: 0, offset: 0 },
            Instruction::StoreWord { s: 3, a: 1, offset: 12 },
            Instruction::StoreWord { s: 28, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 26 },
            Instruction::FloatMultiplySingle { d: 0, a: 20, c: 0 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        ];
        assert!(conversion_packet(&instructions));
        assign_frame(&mut instructions);
        assert!(matches!(instructions[0], Instruction::LoadHalfwordZero { d: 0, .. }));
        assert!(matches!(instructions[1], Instruction::StoreWord { s: 0, offset: 36, .. }));
        assert!(matches!(instructions[2], Instruction::StoreWord { s: 28, offset: 32, .. }));
        assert!(matches!(instructions[7], Instruction::StoreFloatDouble { offset: 24, .. }));
        assert!(matches!(instructions[8], Instruction::LoadWord { offset: 28, .. }));
    }
}
