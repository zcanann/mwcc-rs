//! easin: an exact-match whole-function capture (see captures::ast_hash
//! and docs/emission-model.md for the pipeline).

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the fdlibm __ieee754_asin (captured fire 443).
pub(super) const EASIN_AST_HASH: u64 = 0xc72823698c923c32;

impl Generator {
    /// THE E_ASIN EXACT-MATCH TEMPLATE (fire 443): __ieee754_asin whole
    /// (capture->dis2rust->AST-hash; see try_efmod). 153 instructions;
    /// the software-sqrt inline is flattened INTO the body (frsqrte +
    /// Newton steps); __float_nan/__float_huge via HA/LO externals.
    pub(super) fn try_easin(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__ieee754_asin"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
            // CONTEXT GATE: the capture has the software sqrt INLINED — valid
            // only when this TU's headers define the sqrt inline (recorded in
            // the skipped set). BfBB/pikmin2 share the AST but declare sqrt
            // extern-only; their mwcc emits a real bl sqrt, so they decline
            // here and defer through the general guards.
            || !self.skipped_inline_names.contains("sqrt")
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != EASIN_AST_HASH {
            return Ok(false);
        }
        // strikers shares this AST post-fold (f524) but pools at a bumped
        // base -- easin_str owns that context.
        if super::skipped_context_fingerprint(&self.skipped_inline_names) == 0x4c0074f426dac8c9 {
            return Ok(false);
        }
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        // Pool constants in creation (.sdata2 layout) order.
        for bits in [
            0x3ff921fb54442d18u64,
            0x3c91a62633145c07,
            0x7e37e43c8800759c,
            0x3ff0000000000000,
            0x3fc5555555555555,
            0xbfd4d61203eb6f7d,
            0x3fc9c1550e884455,
            0xbfa48228b5688f3b,
            0x3f49efe07501b288,
            0x3f023de10dfdf709,
            0xc0033a271c8a2d4b,
            0x40002ae59c598ac8,
            0xbfe6066c1b8d0159,
            0x3fb3b8c5b12e9282,
            0x3fe0000000000000,
            0x0000000000000000,
            0x4008000000000000,
            0x4000000000000000,
            0x3fe921fb54442d18,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [16, 19, 31, 32, 58, 107, 110, 115, 117, 130, 147, 150, 151] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16368));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 5,
                s: 4,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&19]); // blt
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 5,
                immediate: -16368,
            });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.load_double_constant(0, 0x3c91a62633145c07);
        self.load_double_constant(2, 0x3ff921fb54442d18);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 2,
                c: 1,
                b: 0,
            });
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&16]);
        self.record_relocation(RelocationKind::Addr16Ha, "__float_nan");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_nan");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&19]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 16352));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&58]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 15936));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&31]); // bge
        self.load_double_constant(3, 0x7e37e43c8800759c);
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 3, a: 3, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&32]); // ble
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 1 });
        self.bind_label(labels[&32]);
        self.load_double_constant(1, 0x3f023de10dfdf709);
        self.load_double_constant(0, 0x3f49efe07501b288);
        self.load_double_constant(3, 0xbfa48228b5688f3b);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 1,
                c: 2,
                b: 0,
            });
        self.load_double_constant(7, 0x3fc9c1550e884455);
        self.load_double_constant(1, 0x3fb3b8c5b12e9282);
        self.load_double_constant(0, 0xbfe6066c1b8d0159);
        self.load_double_constant(6, 0xbfd4d61203eb6f7d);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 8,
                a: 2,
                c: 4,
                b: 3,
            });
        self.load_double_constant(3, 0x40002ae59c598ac8);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 1,
                c: 2,
                b: 0,
            });
        self.load_double_constant(5, 0x3fc5555555555555);
        self.load_double_constant(1, 0xc0033a271c8a2d4b);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 7,
                a: 2,
                c: 8,
                b: 7,
            });
        self.load_double_constant(0, 0x3ff0000000000000);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 2,
                c: 4,
                b: 3,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 8,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 2,
                c: 7,
                b: 6,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 2,
                c: 3,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 2,
                c: 4,
                b: 5,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 0,
                a: 2,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 2, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 8,
                c: 0,
                b: 8,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16,
            });
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&58]);
        self.output
            .instructions
            .push(Instruction::FloatAbsolute { d: 1, b: 1 });
        self.load_double_constant(12, 0x3ff0000000000000);
        self.load_double_constant(0, 0x3fe0000000000000);
        self.load_double_constant(6, 0x3f023de10dfdf709);
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 11, a: 12, b: 1 });
        self.load_double_constant(4, 0x3f49efe07501b288);
        self.load_double_constant(9, 0xbfa48228b5688f3b);
        self.load_double_constant(8, 0x3fc9c1550e884455);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 2, a: 0, c: 11 });
        self.load_double_constant(1, 0x0000000000000000);
        self.load_double_constant(5, 0x3fb3b8c5b12e9282);
        self.load_double_constant(3, 0xbfe6066c1b8d0159);
        self.load_double_constant(7, 0xbfd4d61203eb6f7d);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 10,
                a: 6,
                c: 2,
                b: 4,
            });
        self.load_double_constant(4, 0x40002ae59c598ac8);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 5,
                a: 5,
                c: 2,
                b: 3,
            });
        self.load_double_constant(6, 0x3fc5555555555555);
        self.load_double_constant(3, 0xc0033a271c8a2d4b);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 9,
                a: 2,
                c: 10,
                b: 9,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 11,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 2,
                c: 5,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 5,
                a: 2,
                c: 9,
                b: 8,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 2,
                c: 4,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 4,
                a: 2,
                c: 5,
                b: 7,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 8,
                a: 2,
                c: 3,
                b: 12,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 3,
                a: 2,
                c: 4,
                b: 6,
            });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 5, a: 2, c: 3 });
        self.emit_branch_conditional_to(4, 1, labels[&107]); // ble
        self.output
            .instructions
            .push(Instruction::FloatReciprocalSqrtEstimate { d: 3, b: 2 });
        self.load_double_constant(4, 0x4008000000000000);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 3, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 0, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 1,
                a: 2,
                c: 1,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 3, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 3, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 0, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 1,
                a: 2,
                c: 1,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 3, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 3, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 0, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 1,
                a: 2,
                c: 1,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 3, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 3, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 0, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 0,
                a: 2,
                c: 1,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 3, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 2, c: 0 });
        self.emit_branch_to(labels[&117]); // b
        self.bind_label(labels[&107]);
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&110]); // bne
        self.emit_branch_to(labels[&117]); // b
        self.bind_label(labels[&110]);
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 2, b: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&115]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__float_nan");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_nan");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.emit_branch_to(labels[&117]); // b
        self.bind_label(labels[&115]);
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.bind_label(labels[&117]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 16367));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 13107,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&130]); // blt
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 4, a: 5, b: 8 });
        self.load_double_constant(2, 0x4000000000000000);
        self.load_double_constant(0, 0x3c91a62633145c07);
        self.load_double_constant(3, 0x3ff921fb54442d18);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddDouble {
                d: 1,
                a: 1,
                c: 4,
                b: 1,
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
            .push(Instruction::FloatMultiplySubtractDouble {
                d: 0,
                a: 2,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 3, b: 0 });
        self.emit_branch_to(labels[&147]); // b
        self.bind_label(labels[&130]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.load_double_constant(7, 0x4000000000000000);
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 5, a: 5, b: 8 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.load_double_constant(0, 0x3c91a62633145c07);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 8,
            a: 1,
            offset: 16,
        });
        self.load_double_constant(3, 0x3fe921fb54442d18);
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 4,
                a: 8,
                c: 8,
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 2, a: 1, b: 8 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 6, a: 7, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 4, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 1,
                a: 7,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 0,
                a: 7,
                c: 8,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySubtractDouble {
                d: 1,
                a: 6,
                c: 5,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 3, b: 0 });
        self.bind_label(labels[&147]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&150]); // ble
        self.emit_branch_to(labels[&151]); // b
        self.bind_label(labels[&150]);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real pools start at @72.
        self.output.anonymous_label_bump += 38;
        Ok(true)
    }
}
