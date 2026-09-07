//! Paired conversions and masked inserts for the intrinsic packet profile.

use super::super::*;
use super::MatrixPacket;

impl Generator {
    pub(super) fn emit_intrinsic_matrix_packets(
        &mut self,
        shape: &MatrixPacket<'_>,
    ) -> Compilation<()> {
        self.evaluate(&Expression::FloatLiteral(1024.0), Type::Float, 2)?;
        self.output.instructions.extend([
            Instruction::MultiplyImmediate {
                d: 31,
                a: 0,
                immediate: 3,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 4,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 4,
                offset: 12,
            },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::AddImmediate {
                d: 10,
                a: 5,
                immediate: 17,
            },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::ExtendSignByte { a: 10, s: 10 },
            Instruction::load_immediate(12, shape.command),
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::load_immediate_shifted(11, (shape.port.wrapping_add(0x8000) >> 16) as i16),
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::load_immediate(30, 0),
            Instruction::AddImmediate {
                d: 0,
                a: 31,
                immediate: 6,
            },
            Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 88,
            },
            Instruction::RotateAndMask {
                a: 8,
                s: 10,
                shift: 30,
                begin: 30,
                end: 31,
            },
            Instruction::AddImmediate {
                d: 7,
                a: 31,
                immediate: 7,
            },
        ]);
        self.evaluate(
            &Expression::Variable(shape.global.into()),
            self.globals[shape.global],
            3,
        )?;
        self.output.instructions.extend([
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 80,
            },
            Instruction::LoadWord {
                d: 9,
                a: 1,
                offset: 92,
            },
            Instruction::load_immediate(29, 0),
            Instruction::LoadWord {
                d: 6,
                a: 1,
                offset: 84,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 31,
                immediate: 8,
            },
            Instruction::RotateAndMaskInsert {
                a: 30,
                s: 9,
                shift: 0,
                begin: 21,
                end: 31,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 30,
                immediate: 0,
            },
            Instruction::StoreByte {
                s: 12,
                a: 11,
                offset: shape.port as i16,
            },
            Instruction::RotateAndMaskInsert {
                a: 9,
                s: 6,
                shift: 11,
                begin: 10,
                end: 20,
            },
            Instruction::RotateAndMaskInsert {
                a: 9,
                s: 10,
                shift: 22,
                begin: 8,
                end: 9,
            },
            Instruction::RotateAndMaskInsert {
                a: 9,
                s: 0,
                shift: 24,
                begin: 0,
                end: 7,
            },
            Instruction::StoreWord {
                s: 9,
                a: 11,
                offset: shape.port as i16,
            },
            Instruction::RotateAndMask {
                a: 6,
                s: 10,
                shift: 28,
                begin: 30,
                end: 31,
            },
            Instruction::load_immediate(30, 0),
        ]);
        // Each pair has distinct conversion spill slots. The command write
        // precedes conversion completion, while the data write consumes both.
        for (column, accumulator, first, second, scale, packet) in
            [(1, 29, 10, 9, 8, 7), (2, 30, 7, 4, 6, 5)]
        {
            let spill = 88 - column * 16;
            self.output.instructions.push(Instruction::LoadFloatSingle {
                d: 1,
                a: 4,
                offset: column * 4,
            });
            if column == 1 {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 0));
            }
            self.output.instructions.extend([
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 4,
                    offset: 12 + column * 4,
                },
                Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
                Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
                Instruction::StoreByte {
                    s: 12,
                    a: 11,
                    offset: shape.port as i16,
                },
                Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
                Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
                Instruction::StoreFloatDouble {
                    s: 1,
                    a: 1,
                    offset: spill,
                },
                Instruction::StoreFloatDouble {
                    s: 0,
                    a: 1,
                    offset: spill - 8,
                },
                Instruction::LoadWord {
                    d: first,
                    a: 1,
                    offset: spill + 4,
                },
                Instruction::LoadWord {
                    d: second,
                    a: 1,
                    offset: spill - 4,
                },
                Instruction::RotateAndMaskInsert {
                    a: accumulator,
                    s: first,
                    shift: 0,
                    begin: 21,
                    end: 31,
                },
                Instruction::RotateAndMaskInsert {
                    a: accumulator,
                    s: second,
                    shift: 11,
                    begin: 10,
                    end: 20,
                },
                Instruction::RotateAndMaskInsert {
                    a: accumulator,
                    s: scale,
                    shift: 22,
                    begin: 8,
                    end: 9,
                },
                Instruction::RotateAndMaskInsert {
                    a: accumulator,
                    s: packet,
                    shift: 24,
                    begin: 0,
                    end: 7,
                },
                Instruction::StoreWord {
                    s: accumulator,
                    a: 11,
                    offset: shape.port as i16,
                },
            ]);
        }
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 3,
            offset: shape.flag_offset,
        });
        self.emit_epilogue_and_return();
        Ok(())
    }
}
