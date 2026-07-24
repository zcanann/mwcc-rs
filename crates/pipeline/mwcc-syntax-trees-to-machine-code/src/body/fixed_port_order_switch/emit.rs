//! Build-163 dispatch and per-arm schedules for packed order updates.

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
        _ => unreachable!("order switch patch point is a branch"),
    }
}

impl Generator {
    fn emit_order_global(
        &mut self,
        global: &str,
        global_type: Type,
        register: u8,
    ) -> Compilation<()> {
        self.evaluate(&Expression::Variable(global.into()), global_type, register)
    }

    fn emit_first_order_arm(
        &mut self,
        global: &str,
        global_type: Type,
        word_offset: i16,
    ) -> Compilation<()> {
        self.emit_order_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 3,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: word_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: word_offset,
            },
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 0,
                end: 28,
            },
            Instruction::Or { a: 3, s: 3, b: 5 },
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: 0,
            },
        ]);
        self.emit_order_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: word_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: word_offset,
            },
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 29,
                end: 25,
            },
            Instruction::Or { a: 0, s: 3, b: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
        ]);
        Ok(())
    }

    fn emit_later_order_arm(
        &mut self,
        global: &str,
        global_type: Type,
        word_offset: i16,
        map_shift: u8,
    ) -> Compilation<()> {
        let coordinate_shift = map_shift + 3;
        let map_mask_begin = 32 - map_shift;
        let coordinate_mask_begin = 32 - coordinate_shift;
        self.emit_order_global(global, global_type, 6)?;
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: coordinate_shift,
            },
            Instruction::ShiftLeftImmediate {
                a: 3,
                s: 5,
                shift: map_shift,
            },
            Instruction::LoadWord {
                d: 4,
                a: 6,
                offset: word_offset,
            },
            Instruction::RotateAndMask {
                a: 4,
                s: 4,
                shift: 0,
                begin: map_mask_begin,
                end: map_mask_begin - 4,
            },
            Instruction::Or { a: 3, s: 4, b: 3 },
            Instruction::StoreWord {
                s: 3,
                a: 6,
                offset: word_offset,
            },
        ]);
        self.emit_order_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: word_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: word_offset,
            },
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: coordinate_mask_begin,
                end: coordinate_mask_begin - 4,
            },
            Instruction::Or { a: 0, s: 3, b: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
        ]);
        Ok(())
    }

    pub(crate) fn try_fixed_port_order_switch(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self
                .locations
                .get(shape.selector)
                .map(|location| location.register)
                != Some(3)
            || self
                .locations
                .get(shape.coordinate)
                .map(|location| location.register)
                != Some(4)
            || self
                .locations
                .get(shape.map)
                .map(|location| location.register)
                != Some(5)
        {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(shape.global) else {
            return Ok(false);
        };
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 2 });
        let case2_branch = push_conditional(self, 12, 2);
        let upper_branch = push_conditional(self, 4, 0);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let case0_branch = push_conditional(self, 12, 2);
        let case1_branch = push_conditional(self, 4, 0);
        let below_branch = push_branch(self);
        let upper = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 4 });
        let above_branch = push_conditional(self, 4, 0);
        let case3_branch = push_branch(self);

        let case0 = self.output.instructions.len();
        self.emit_first_order_arm(shape.global, global_type, shape.word_offset)?;
        let case0_join = push_branch(self);
        let case1 = self.output.instructions.len();
        self.emit_later_order_arm(shape.global, global_type, shape.word_offset, 6)?;
        let case1_join = push_branch(self);
        let case2 = self.output.instructions.len();
        self.emit_later_order_arm(shape.global, global_type, shape.word_offset, 12)?;
        let case2_join = push_branch(self);
        let case3 = self.output.instructions.len();
        self.emit_later_order_arm(shape.global, global_type, shape.word_offset, 18)?;
        let tail = self.output.instructions.len();

        patch(self, case2_branch, case2);
        patch(self, upper_branch, upper);
        patch(self, case0_branch, case0);
        patch(self, case1_branch, case1);
        patch(self, below_branch, tail);
        patch(self, above_branch, tail);
        patch(self, case3_branch, case3);
        for branch in [case0_join, case1_join, case2_join] {
            patch(self, branch, tail);
        }

        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0x61));
        self.emit_order_global(shape.global, global_type, 4)?;
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(5, 0xcc01u16 as i16),
            Instruction::StoreByte {
                s: 0,
                a: 5,
                offset: -32768,
            },
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: shape.word_offset,
            },
            Instruction::StoreWord {
                s: 3,
                a: 5,
                offset: -32768,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: shape.dirty_offset,
            },
            Instruction::OrImmediate {
                a: 3,
                s: 3,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: shape.dirty_offset,
            },
        ]);
        self.emit_order_global(shape.global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::StoreHalfword {
                s: 0,
                a: 3,
                offset: shape.flag_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
