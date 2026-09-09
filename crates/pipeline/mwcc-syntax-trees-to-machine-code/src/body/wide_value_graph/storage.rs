//! Scalar storage widths used alongside explicit word/pair values.
use super::*;

pub(super) fn narrow_constant(bits: u64, ty: Type) -> u64 {
    match ty {
        Type::Char => (bits as i8 as i32 as u32) as u64,
        Type::UnsignedChar => bits & 0xff,
        Type::Short => (bits as i16 as i32 as u32) as u64,
        Type::UnsignedShort => bits & 0xffff,
        _ => bits,
    }
}

impl Generator {
    pub(super) fn wide_graph_narrow(&mut self, ty: Type, destination: u32, source: u32) {
        self.output.instructions.push(match ty {
            Type::Char => Instruction::ExtendSignByte {
                a: destination,
                s: source,
            },
            Type::Short => Instruction::ExtendSignHalfword {
                a: destination,
                s: source,
            },
            Type::UnsignedChar | Type::UnsignedShort => Instruction::AndContiguousMask {
                a: destination,
                s: source,
                begin: 32 - ty.width(),
                end: 31,
            },
            _ => Instruction::move_register(destination, source),
        });
    }

    pub(super) fn wide_graph_scalar_load(&mut self, ty: Type, destination: u32, pointer: u32) {
        self.output.instructions.push(match ty {
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero {
                d: destination,
                a: pointer,
                offset: 0,
            },
            Type::Short => Instruction::LoadHalfwordAlgebraic {
                d: destination,
                a: pointer,
                offset: 0,
            },
            Type::UnsignedShort => Instruction::LoadHalfwordZero {
                d: destination,
                a: pointer,
                offset: 0,
            },
            _ => Instruction::LoadWord {
                d: destination,
                a: pointer,
                offset: 0,
            },
        });
        if ty == Type::Char {
            self.output.instructions.push(Instruction::ExtendSignByte {
                a: destination,
                s: destination,
            });
        }
    }

    pub(super) fn wide_graph_scalar_store(
        &mut self,
        ty: Type,
        source: u32,
        pointer: u32,
        offset: i16,
    ) {
        self.output.instructions.push(match ty {
            Type::Char | Type::UnsignedChar => Instruction::StoreByte {
                s: source,
                a: pointer,
                offset,
            },
            Type::Short | Type::UnsignedShort => Instruction::StoreHalfword {
                s: source,
                a: pointer,
                offset,
            },
            _ => Instruction::StoreWord {
                s: source,
                a: pointer,
                offset,
            },
        });
    }
}
