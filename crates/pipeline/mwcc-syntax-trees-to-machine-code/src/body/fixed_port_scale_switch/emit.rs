//! Build-163 dispatch and per-arm schedules for scale updates.

use super::recognize::recognize;
#[allow(unused_imports)]
use super::super::*;

fn push_conditional(generator: &mut Generator, options: u8, condition_bit: u8) -> usize {
    let index = generator.output.instructions.len();
    generator.output.instructions.push(Instruction::BranchConditionalForward {
        options,
        condition_bit,
        target: 0,
    });
    index
}

fn push_branch(generator: &mut Generator) -> usize {
    let index = generator.output.instructions.len();
    generator.output.instructions.push(Instruction::Branch { target: 0 });
    index
}

fn patch(generator: &mut Generator, index: usize, target_index: usize) {
    match &mut generator.output.instructions[index] {
        Instruction::BranchConditionalForward { target, .. }
        | Instruction::Branch { target } => *target = target_index,
        _ => unreachable!("scale switch patch point is a branch"),
    }
}

impl Generator {
    fn emit_scale_global(&mut self, global: &str, global_type: Type, register: u8) -> Compilation<()> {
        self.evaluate(&Expression::Variable(global.into()), global_type, register)
    }

    fn emit_low_scale_arm(
        &mut self,
        global: &str,
        global_type: Type,
        member_offset: i16,
        command: u16,
    ) -> Compilation<()> {
        self.emit_scale_global(global, global_type, 6)?;
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 3, s: 5, shift: 4 },
            Instruction::load_immediate(0, 0x61),
            Instruction::AddImmediate { d: 7, a: 6, immediate: member_offset },
            Instruction::LoadWord { d: 6, a: 6, offset: member_offset },
            Instruction::load_immediate_shifted(5, 0xcc01u16 as i16),
            Instruction::RotateAndMask { a: 6, s: 6, shift: 0, begin: 0, end: 27 },
            Instruction::Or { a: 4, s: 6, b: 4 },
            Instruction::StoreWord { s: 4, a: 7, offset: 0 },
        ]);
        self.emit_scale_global(global, global_type, 4)?;
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 6, a: 4, immediate: member_offset },
            Instruction::LoadWord { d: 4, a: 4, offset: member_offset },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 0, begin: 28, end: 23 },
            Instruction::Or { a: 3, s: 4, b: 3 },
            Instruction::StoreWord { s: 3, a: 6, offset: 0 },
        ]);
        self.emit_scale_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 3, immediate: member_offset },
            Instruction::LoadWord { d: 3, a: 3, offset: member_offset },
            Instruction::RotateAndMask { a: 3, s: 3, shift: 0, begin: 8, end: 31 },
            Instruction::OrImmediateShifted { a: 3, s: 3, immediate: command << 8 },
            Instruction::StoreWord { s: 3, a: 4, offset: 0 },
            Instruction::StoreByte { s: 0, a: 5, offset: -32768 },
        ]);
        self.emit_scale_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 3, offset: member_offset },
            Instruction::StoreWord { s: 0, a: 5, offset: -32768 },
        ]);
        Ok(())
    }

    fn emit_high_scale_arm(
        &mut self,
        global: &str,
        global_type: Type,
        member_offset: i16,
        command: u16,
    ) -> Compilation<()> {
        self.emit_scale_global(global, global_type, 7)?;
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 3, s: 5, shift: 12 },
            Instruction::LoadWordWithUpdate { d: 5, a: 7, offset: member_offset },
            Instruction::ShiftLeftImmediate { a: 6, s: 4, shift: 8 },
            Instruction::load_immediate(0, 0x61),
            Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 24, end: 19 },
            Instruction::Or { a: 5, s: 5, b: 6 },
            Instruction::StoreWord { s: 5, a: 7, offset: 0 },
            Instruction::load_immediate_shifted(4, 0xcc01u16 as i16),
        ]);
        self.emit_scale_global(global, global_type, 5)?;
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 6, a: 5, immediate: member_offset },
            Instruction::LoadWord { d: 5, a: 5, offset: member_offset },
            Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 20, end: 15 },
            Instruction::Or { a: 3, s: 5, b: 3 },
            Instruction::StoreWord { s: 3, a: 6, offset: 0 },
        ]);
        self.emit_scale_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 3, immediate: member_offset },
            Instruction::LoadWord { d: 3, a: 3, offset: member_offset },
            Instruction::RotateAndMask { a: 3, s: 3, shift: 0, begin: 8, end: 31 },
            Instruction::OrImmediateShifted { a: 3, s: 3, immediate: command << 8 },
            Instruction::StoreWord { s: 3, a: 5, offset: 0 },
            Instruction::StoreByte { s: 0, a: 4, offset: -32768 },
        ]);
        self.emit_scale_global(global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 3, offset: member_offset },
            Instruction::StoreWord { s: 0, a: 4, offset: -32768 },
        ]);
        Ok(())
    }

    pub(crate) fn try_fixed_port_scale_switch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.locations.get(shape.selector).map(|location| location.register) != Some(3)
            || self.locations.get(shape.first_scale).map(|location| location.register) != Some(4)
            || self.locations.get(shape.second_scale).map(|location| location.register) != Some(5)
        {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(shape.global) else {
            return Ok(false);
        };
        self.output.pre_scheduled = true;
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 2 });
        let case2_branch = push_conditional(self, 12, 2);
        let upper_branch = push_conditional(self, 4, 0);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let case0_branch = push_conditional(self, 12, 2);
        let case1_branch = push_conditional(self, 4, 0);
        let below_branch = push_branch(self);
        let upper = self.output.instructions.len();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 4 });
        let above_branch = push_conditional(self, 4, 0);
        let case3_branch = push_branch(self);

        let case0 = self.output.instructions.len();
        self.emit_low_scale_arm(shape.global, global_type, shape.first_offset, 0x25)?;
        let case0_join = push_branch(self);
        let case1 = self.output.instructions.len();
        self.emit_high_scale_arm(shape.global, global_type, shape.first_offset, 0x25)?;
        let case1_join = push_branch(self);
        let case2 = self.output.instructions.len();
        self.emit_low_scale_arm(shape.global, global_type, shape.second_offset, 0x26)?;
        let case2_join = push_branch(self);
        let case3 = self.output.instructions.len();
        self.emit_high_scale_arm(shape.global, global_type, shape.second_offset, 0x26)?;
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

        self.emit_scale_global(shape.global, global_type, 3)?;
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword { s: 0, a: 3, offset: shape.flag_offset },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
