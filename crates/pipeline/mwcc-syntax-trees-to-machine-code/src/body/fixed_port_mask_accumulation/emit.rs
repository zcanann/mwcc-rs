//! Build-163 counted-loop and conditional replay schedule.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

fn push_conditional(generator: &mut Generator, options: u8, condition_bit: u8) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::BranchConditionalForward {
            options,
            condition_bit,
            target: 0,
        });
    index
}

fn push_branch(generator: &mut Generator) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::Branch { target: 0 });
    index
}

fn patch(generator: &mut Generator, index: usize, target_index: usize) {
    match &mut generator.output.instructions[index] {
        Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target } => {
            *target = target_index
        }
        _ => unreachable!("mask accumulation patch point is a branch"),
    }
}

impl Generator {
    pub(crate) fn try_fixed_port_mask_accumulation(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(shape.global) else {
            return Ok(false);
        };
        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 3)?;
        self.output.instructions.extend([
            Instruction::load_immediate(6, 0),
            Instruction::load_immediate(4, 0),
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.count_offset,
            },
            Instruction::RotateAndMask {
                a: 0,
                s: 0,
                shift: 16,
                begin: 29,
                end: 31,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::MoveToCountRegister { s: 0 },
        ]);
        let empty_branch = push_conditional(self, 4, 1);
        let loop_start = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 2 });
        let case2_branch = push_conditional(self, 12, 2);
        let upper_branch = push_conditional(self, 4, 0);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        let case0_branch = push_conditional(self, 12, 2);
        let case1_branch = push_conditional(self, 4, 0);
        let default_low = push_branch(self);
        let upper = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 4 });
        let default_high = push_conditional(self, 4, 0);
        let case3_branch = push_branch(self);

        let case0 = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.packed_offset,
            },
            Instruction::ClearLeftImmediate {
                a: 5,
                s: 0,
                clear: 29,
            },
        ]);
        let case0_join = push_branch(self);
        let case1 = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.packed_offset,
            },
            Instruction::RotateAndMask {
                a: 5,
                s: 0,
                shift: 26,
                begin: 29,
                end: 31,
            },
        ]);
        let case1_join = push_branch(self);
        let case2 = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.packed_offset,
            },
            Instruction::RotateAndMask {
                a: 5,
                s: 0,
                shift: 20,
                begin: 29,
                end: 31,
            },
        ]);
        let case2_join = push_branch(self);
        let case3 = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.packed_offset,
            },
            Instruction::RotateAndMask {
                a: 5,
                s: 0,
                shift: 14,
                begin: 29,
                end: 31,
            },
        ]);
        let join = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::load_immediate(0, 1),
            Instruction::ShiftLeftWord { a: 0, s: 0, b: 5 },
            Instruction::Or { a: 6, s: 6, b: 0 },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 1,
            },
        ]);
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: loop_start,
            });
        let tail = self.output.instructions.len();

        patch(self, empty_branch, tail);
        patch(self, case2_branch, case2);
        patch(self, upper_branch, upper);
        patch(self, case0_branch, case0);
        patch(self, case1_branch, case1);
        patch(self, default_low, join);
        patch(self, default_high, join);
        patch(self, case3_branch, case3);
        for branch in [case0_join, case1_join, case2_join] {
            patch(self, branch, join);
        }

        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: shape.mask_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: shape.mask_offset,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 3,
                clear: 24,
            },
            Instruction::CompareLogicalWord { a: 0, b: 6 },
            Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            },
            Instruction::RotateAndMask {
                a: 0,
                s: 3,
                shift: 0,
                begin: 0,
                end: 23,
            },
            Instruction::Or { a: 0, s: 0, b: 6 },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
            Instruction::load_immediate(0, 0x61),
            Instruction::load_immediate_shifted(5, 0xcc01u16 as i16),
            Instruction::StoreByte {
                s: 0,
                a: 5,
                offset: -32768,
            },
            Instruction::load_immediate(0, 0),
        ]);
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 4)?;
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: shape.mask_offset,
            },
            Instruction::StoreWord {
                s: 3,
                a: 5,
                offset: -32768,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: shape.flag_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
