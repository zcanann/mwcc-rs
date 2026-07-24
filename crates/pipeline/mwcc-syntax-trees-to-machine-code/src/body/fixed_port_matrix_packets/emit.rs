//! Register scheduling for recognized scaled matrix packet runs.

use super::recognize;
#[allow(unused_imports)]
use super::super::*;

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

fn patch_branch(generator: &mut Generator, index: usize, destination: usize) {
    match &mut generator.output.instructions[index] {
        Instruction::BranchConditionalForward { target, .. }
        | Instruction::Branch { target } => *target = destination,
        _ => unreachable!("packet dispatch patch points are branches"),
    }
}

impl Generator {
    pub(crate) fn try_fixed_port_matrix_packets(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.locations.get(shape.matrix_id).map(|location| location.register) != Some(3)
            || self.locations.get(shape.source).map(|location| location.register) != Some(4)
            || self.locations.get(shape.scale).map(|location| location.register) != Some(5)
            || self.globals.get(shape.global).is_none()
        {
            return Ok(false);
        }
        let _semantic_locals = (shape.values, shape.word, shape.packet_id);
        self.output.pre_scheduled = true;
        self.output.has_conversion = true;
        self.frame_size = 120;

        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -120 });
        let beq8 = push_conditional(self, 12, 2);
        let bge8 = push_conditional(self, 4, 0);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 4 });
        let beq4 = push_conditional(self, 12, 2);
        let bge4 = push_conditional(self, 4, 0);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        let bge1 = push_conditional(self, 4, 0);
        let below1 = push_branch(self);
        let cmp12 = self.output.instructions.len();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 12 });
        let bge12 = push_conditional(self, 4, 0);
        let below12 = push_branch(self);
        let sub1 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        let join1 = push_branch(self);
        let sub5 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -5 });
        let join5 = push_branch(self);
        let sub9 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -9 });
        let join9 = push_branch(self);
        let default = self.output.instructions.len();
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        let join = self.output.instructions.len();
        for branch in [beq8, beq4, below1, bge12] {
            patch_branch(self, branch, default);
        }
        patch_branch(self, bge8, cmp12);
        patch_branch(self, bge4, sub5);
        patch_branch(self, bge1, sub1);
        patch_branch(self, below12, sub9);
        for branch in [join1, join5, join9] {
            patch_branch(self, branch, join);
        }

        self.evaluate(&Expression::FloatLiteral(1024.0), Type::Float, 2)?;
        self.output.instructions.extend([
            Instruction::MultiplyImmediate { d: 3, a: 0, immediate: 3 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 12 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::AddImmediate { d: 11, a: 5, immediate: 17 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 6 },
            Instruction::ExtendSignByte { a: 11, s: 11 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::load_immediate(10, 0x61),
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::load_immediate_shifted(9, 0xcc01u16 as i16),
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 112 },
            Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 24 },
            Instruction::AddImmediate { d: 6, a: 3, immediate: 7 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 104 },
            Instruction::AddImmediate { d: 5, a: 3, immediate: 8 },
            Instruction::LoadWord { d: 8, a: 1, offset: 116 },
            Instruction::LoadWord { d: 7, a: 1, offset: 108 },
        ]);
        let global_type = self.globals[shape.global];
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 3)?;
        self.output.instructions.extend([
            Instruction::RotateAndMask { a: 7, s: 7, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 7, s: 8, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 7, s: 11, shift: 22, begin: 8, end: 9 },
            Instruction::RotateAndMaskInsert { a: 0, s: 7, shift: 0, begin: 8, end: 31 },
            Instruction::StoreWord { s: 0, a: 9, offset: -32768 },
            Instruction::load_immediate(0, 0),
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 4 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 16 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 96 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 88 },
            Instruction::LoadWord { d: 8, a: 1, offset: 100 },
            Instruction::LoadWord { d: 7, a: 1, offset: 92 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 7, s: 8, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 7, s: 11, shift: 20, begin: 8, end: 9 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 8, end: 31 },
            Instruction::RotateAndMaskInsert { a: 7, s: 6, shift: 24, begin: 0, end: 7 },
            Instruction::StoreWord { s: 7, a: 9, offset: -32768 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 8 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 20 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 80 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 72 },
            Instruction::LoadWord { d: 6, a: 1, offset: 84 },
            Instruction::LoadWord { d: 4, a: 1, offset: 76 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 4, s: 6, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 4, s: 11, shift: 18, begin: 8, end: 9 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 0, begin: 8, end: 31 },
            Instruction::RotateAndMaskInsert { a: 4, s: 5, shift: 24, begin: 0, end: 7 },
            Instruction::StoreWord { s: 4, a: 9, offset: -32768 },
            Instruction::StoreHalfword { s: 0, a: 3, offset: shape.flag_offset },
        ]);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
