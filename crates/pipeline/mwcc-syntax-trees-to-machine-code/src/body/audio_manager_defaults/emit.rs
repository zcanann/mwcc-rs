use super::*;

pub(super) fn emit(generator: &mut Generator) {
    generator.output.pre_scheduled = true;
    generator.output.instructions.extend([
        Instruction::load_immediate(6, 0),
        Instruction::load_immediate(5, 1),
        Instruction::StoreWord { s: 6, a: 3, offset: 8 },
        Instruction::load_immediate(0, 8),
        Instruction::AddImmediate { d: 4, a: 6, immediate: 0 },
        Instruction::StoreWord { s: 6, a: 3, offset: 12 },
        Instruction::StoreWord { s: 6, a: 3, offset: 16 },
        Instruction::StoreWord { s: 6, a: 3, offset: 20 },
        Instruction::StoreWord { s: 6, a: 3, offset: 4 },
        Instruction::StoreWord { s: 6, a: 3, offset: 0 },
        Instruction::StoreWord { s: 5, a: 3, offset: 112 },
    ]);
    generator.load_float_constant(0, 1.0);
    generator.output.instructions.extend([
        Instruction::StoreFloatSingle { s: 0, a: 3, offset: 24 },
        Instruction::StoreFloatSingle { s: 0, a: 3, offset: 28 },
    ]);
    generator.load_float_constant(0, 0.5);
    generator.output.instructions.push(Instruction::StoreFloatSingle {
        s: 0,
        a: 3,
        offset: 32,
    });
    generator.load_float_constant(0, 0.0);
    generator.output.instructions.extend([
        Instruction::StoreFloatSingle { s: 0, a: 3, offset: 36 },
        Instruction::StoreFloatSingle { s: 0, a: 3, offset: 40 },
        Instruction::MoveToCountRegister { s: 0 },
        Instruction::AddImmediate { d: 0, a: 4, immediate: 44 },
        Instruction::AddImmediate { d: 4, a: 4, immediate: 2 },
        Instruction::StoreHalfwordIndexed { s: 6, a: 3, b: 0 },
        Instruction::BranchConditionalForward {
            options: 16,
            condition_bit: 0,
            target: 20,
        },
        Instruction::load_immediate(0, 32767),
        Instruction::load_immediate(6, 0),
        Instruction::StoreHalfword { s: 0, a: 3, offset: 44 },
        Instruction::load_immediate(0, 4),
        Instruction::AddImmediate { d: 7, a: 6, immediate: 0 },
        Instruction::load_immediate(4, 0),
        Instruction::StoreHalfword { s: 6, a: 3, offset: 76 },
        Instruction::MoveToCountRegister { s: 0 },
        Instruction::AddImmediate { d: 5, a: 4, immediate: 60 },
        Instruction::AddImmediate { d: 0, a: 7, immediate: 90 },
        Instruction::StoreHalfwordIndexed { s: 6, a: 3, b: 5 },
        Instruction::AddImmediate { d: 7, a: 7, immediate: 1 },
        Instruction::AddImmediate { d: 4, a: 4, immediate: 2 },
        Instruction::StoreByteIndexed { s: 6, a: 3, b: 0 },
        Instruction::BranchConditionalForward {
            options: 16,
            condition_bit: 0,
            target: 32,
        },
        Instruction::load_immediate(11, 0),
        Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 2 },
        Instruction::StoreByte { s: 11, a: 3, offset: 96 },
        Instruction::load_immediate(0, 32767),
        Instruction::load_immediate(10, 336),
        Instruction::load_immediate(9, 528),
        Instruction::StoreHalfword { s: 0, a: 3, offset: 60 },
        Instruction::load_immediate(8, 850),
        Instruction::load_immediate(7, 1042),
        Instruction::AddImmediate { d: 6, a: 4, immediate: 259 },
        Instruction::StoreByte { s: 11, a: 3, offset: 97 },
        Instruction::load_immediate(5, 600),
        Instruction::load_immediate(4, 26),
        Instruction::load_immediate(0, 1),
        Instruction::StoreHalfword { s: 10, a: 3, offset: 78 },
        Instruction::StoreHalfword { s: 9, a: 3, offset: 80 },
        Instruction::StoreHalfword { s: 8, a: 3, offset: 82 },
        Instruction::StoreHalfword { s: 7, a: 3, offset: 84 },
        Instruction::StoreHalfword { s: 11, a: 3, offset: 86 },
        Instruction::StoreHalfword { s: 11, a: 3, offset: 88 },
        Instruction::StoreWord { s: 6, a: 3, offset: 104 },
        Instruction::StoreHalfword { s: 5, a: 3, offset: 108 },
        Instruction::StoreByte { s: 4, a: 3, offset: 98 },
        Instruction::StoreByte { s: 0, a: 3, offset: 99 },
        Instruction::StoreByte { s: 0, a: 3, offset: 100 },
        Instruction::BranchToLinkRegister,
    ]);
}
