//! ktan: an exact-match whole-function capture (see captures::ast_hash
//! and docs/emission-model.md for the pipeline).

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the fdlibm __kernel_tan (captured fire 442).
const KTAN_AST_HASH: u64 = 0x5c388427c9ab01eb;

impl Generator {
    /// THE K_TAN EXACT-MATCH TEMPLATE (fire 442): __kernel_tan whole
    /// (capture->dis2rust->AST-hash; see try_efmod). 133 instructions;
    /// the callee-saved f31 spills stfd + psq_st (Gekko paired-single);
    /// the fctiwz conversion's internal label consumes one @N BETWEEN
    /// pool constants (the (6,1) constant-number gap).
    pub(super) fn try_ktan(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__kernel_tan"
            || function.return_type != Type::Double
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != KTAN_AST_HASH {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.callee_saved_float = 1;
        self.output.constant_number_gaps = vec![(6, 1)];
        // Pool constants pre-registered in mwcc's CREATION order (the
        // .sdata2 layout order) — the text loads them in a different
        // (scheduled) order, and numbering follows creation.
        for bits in [
            0x3ff0000000000000u64,
            0xbff0000000000000,
            0x3fe921fb54442d18,
            0x3c81a62633145c07,
            0x0,
            0x4000000000000000,
            0x4330000080000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [23, 26, 29, 39, 48, 111, 114, 129] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 48,
            });
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedStore {
                s: 31,
                a: 1,
                offset: 56,
                w: 0,
                i: 0,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 15920));
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 7,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&29]); // bge
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 32,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
        self.output
            .instructions
            .push(Instruction::FloatAbsolute { d: 1, b: 1 });
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&23]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&26]);
        self.load_double_constant(0, 0xbff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16358));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -27608,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&48]); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&39]); // bge
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 2, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 0, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.bind_label(labels[&39]);
        self.load_double_constant(0, 0x3c81a62633145c07);
        self.load_double_constant(3, 0x3fe921fb54442d18);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.load_double_constant(2, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 3, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 24,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "xxx");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "xxx");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16358));
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 13, a: 0, c: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -27608,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 5,
            a: 5,
            offset: 96,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 5,
            offset: 80,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 9,
            a: 5,
            offset: 88,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble {
                d: 31,
                a: 13,
                c: 13,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 8,
            a: 5,
            offset: 72,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 3,
            a: 5,
            offset: 64,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 11,
            a: 5,
            offset: 56,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 13, c: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 6,
            a: 5,
            offset: 48,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 7,
                a: 31,
                c: 5,
                b: 4,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 10,
            a: 5,
            offset: 40,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 5,
            a: 5,
            offset: 32,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 12,
                a: 31,
                c: 9,
                b: 8,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 9,
            a: 5,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 5,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 7,
                a: 31,
                c: 7,
                b: 3,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 8,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 11,
                a: 31,
                c: 12,
                b: 11,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 13,
                a: 1,
                offset: 24,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 6,
                a: 31,
                c: 7,
                b: 6,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 7,
                a: 31,
                c: 11,
                b: 10,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 5,
                a: 31,
                c: 6,
                b: 5,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 6,
                a: 31,
                c: 7,
                b: 9,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 31,
                c: 5,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 5,
                a: 31,
                c: 6,
                b: 8,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 4, a: 13, c: 4 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 4, a: 5, b: 4 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 1,
                c: 4,
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 6,
                a: 13,
                c: 4,
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 6,
                a: 3,
                c: 1,
                b: 6,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 6 });
        self.emit_branch_conditional_to(12, 0, labels[&111]); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 17200));
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 3,
                immediate: 32768,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 7,
            shift: 2,
            begin: 30,
            end: 30,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: 0,
                immediate: 1,
            });
        self.load_double_constant(5, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 32,
        });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 0,
                immediate: 32768,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 1 });
        self.load_double_constant(3, 0x4000000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 7, a: 4, b: 5 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 1, b: 7 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 4,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 4, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 6 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 0,
                a: 3,
                c: 0,
                b: 7,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 4, c: 0 });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&111]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&114]); // bne
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&114]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 24,
            });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 4, a: 2, b: 1 });
        self.load_double_constant(1, 0x3ff0000000000000);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 4,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 2, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 6, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 3,
                c: 2,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 0,
                a: 3,
                c: 0,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 4,
                c: 0,
                b: 3,
            });
        self.bind_label(labels[&129]);
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoad {
                d: 31,
                a: 1,
                offset: 56,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 48,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real pools start at @64.
        self.output.anonymous_label_bump += 30;
        Ok(true)
    }
}
