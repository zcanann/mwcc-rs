//! Build-163 leaf frame and loop schedule for quadratic float maps.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

impl Generator {
    pub(crate) fn try_inlined_quadratic_float_map_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.optimization != mwcc_versions::Optimization::O0
            || self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.read_only_global_addressing != GlobalAddressing::Absolute
            || !self.frame_slots.is_empty()
            || !self.output.instructions.is_empty()
            || plan
                .inputs
                .iter()
                .zip([3, 4, 5])
                .any(|(name, register)| self.lookup_general(name) != Some(register))
            || self.lookup_general(plan.output) != Some(6)
            || self.float_register_of(plan.weight).ok() != Some(1)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.frame_size = 144;
        self.callee_saved = vec![31];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -144,
            });
        for (register, double_offset) in [
            (31, 128),
            (30, 112),
            (29, 96),
            (28, 80),
            (27, 64),
            (26, 48),
            (25, 32),
            (24, 16),
        ] {
            self.output.instructions.extend([
                Instruction::StoreFloatDouble {
                    s: register,
                    a: 1,
                    offset: double_offset,
                },
                Instruction::PairedSingleQuantizedStore {
                    s: register,
                    a: 1,
                    offset: double_offset + 8,
                    w: 0,
                    i: 0,
                },
            ]);
        }
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::load_immediate(31, 0),
            Instruction::Branch { target: 50 },
            Instruction::LoadFloatSingle {
                d: 27,
                a: 5,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 4,
            },
            Instruction::LoadFloatSingle {
                d: 28,
                a: 4,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 4,
            },
            Instruction::LoadFloatSingle {
                d: 29,
                a: 3,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 4,
            },
            Instruction::FloatMove { d: 26, b: 29 },
        ]);

        let prior_constant_home = self.structured_constant_address_home.replace(7);
        self.load_double_constant(0, 1.0f64.to_bits());
        self.output.instructions.extend([
            Instruction::FloatSubtractDouble { d: 31, a: 0, b: 1 },
            Instruction::RoundToSingle { d: 31, b: 31 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 1 },
            Instruction::FloatMultiplySingle { d: 4, a: 27, c: 0 },
            Instruction::FloatMultiplySingle { d: 0, a: 31, c: 31 },
            Instruction::FloatMultiplySingle { d: 3, a: 29, c: 0 },
        ]);
        self.load_double_constant(2, 2.0f64.to_bits());
        self.structured_constant_address_home = prior_constant_home;
        self.output.instructions.extend([
            Instruction::FloatMultiplySingle { d: 0, a: 31, c: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 28, c: 0 },
            Instruction::FloatMultiplyDouble { d: 0, a: 2, c: 0 },
            Instruction::FloatAddDouble { d: 0, a: 3, b: 0 },
            Instruction::FloatAddDouble { d: 30, a: 4, b: 0 },
            Instruction::RoundToSingle { d: 30, b: 30 },
            Instruction::FloatMove { d: 25, b: 30 },
            Instruction::FloatMove { d: 24, b: 25 },
            Instruction::StoreFloatSingle {
                s: 24,
                a: 6,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 6,
                immediate: 4,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 31,
                immediate: 1,
            },
            Instruction::CompareWordImmediate {
                a: 31,
                immediate: 3,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 20,
            },
        ]);
        for (register, double_offset) in [
            (31, 128),
            (30, 112),
            (29, 96),
            (28, 80),
            (27, 64),
            (26, 48),
            (25, 32),
            (24, 16),
        ] {
            self.output.instructions.extend([
                Instruction::PairedSingleQuantizedLoad {
                    d: register,
                    a: 1,
                    offset: double_offset + 8,
                    w: 0,
                    i: 0,
                },
                Instruction::LoadFloatDouble {
                    d: register,
                    a: 1,
                    offset: double_offset,
                },
            ]);
        }
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 144,
            },
            Instruction::BranchToLinkRegister,
        ]);
        // The source for-loop contributes the same five anonymous branch
        // ordinals even though this owner emits its control flow directly.
        self.output.anonymous_label_bump += 5;
        Ok(true)
    }
}
