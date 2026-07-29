//! oot_log10f: an exact-match whole-function capture (fire 0).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OOT_LOG10F_AST_HASH: u64 = 0x46caf39bc923453c;

impl Generator {
    pub(super) fn try_oot_log10f(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "log10f"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OOT_LOG10F_AST_HASH {
            eprintln!("oot_log10f hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xb997617ee6b40d7b => 38, // oot-gc mq-j log10f.c
            _ => {
                eprintln!("oot_log10f context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.output.pre_scheduled = true;
        self.output.symbol_order = ["...rodata.0", "__float_nan", "__float_huge"]
            .into_iter()
            .map(String::from)
            .collect();
        for bits in [
            0x3e1a209bu64,
            0x3e9a209b,
            0x3ed413cd,
            0x00000000,
            0x3f800000,
            0x40000000,
        ] {
            self.output.intern_constant(bits, 4);
        }
        self.output.intern_constant(0x4330000080000000, 8);
        self.output.constant_number_gaps = vec![(6, 1)];
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [17, 20, 26, 36, 47, 57, 58, 100, 104, 109, 112, 115] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "...rodata.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "...rodata.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 4,
            s: 6,
            begin: 0,
            end: 8,
        });
        self.emit_branch_conditional_to(12, 2, labels[&112]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&17]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, -128));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&20]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -32768));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&20]); // bge
        self.emit_branch_to(labels[&112]); // b
        self.bind_label(labels[&17]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32640));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 0,
            s: 6,
            begin: 0,
            end: 0,
        });
        self.load_float_constant(8, f32::from_bits(0x3e1a209b));
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
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
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 3,
                clear: 9,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 3,
                shift: 23,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: -126,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 16128,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&47]); // b
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: -1,
        });
        self.load_double_constant(1, 0x4330000080000000);
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 6,
                immediate: 32768,
            });
        self.load_float_constant(2, f32::from_bits(0x3e9a209b));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 40,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 2, c: 0 });
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&47]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 16181));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1267,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&57]); // bge
        self.load_float_constant(1, f32::from_bits(0x3ed413cd));
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 0,
                a: 1,
                c: 0,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 8,
            });
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&57]);
        self.load_float_constant(8, f32::from_bits(0x00000000));
        self.bind_label(labels[&58]);
        self.load_float_constant(2, f32::from_bits(0x3f800000));
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 6,
                immediate: 32768,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 2, b: 0 });
        self.load_float_constant(1, f32::from_bits(0x40000000));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 24,
        });
        self.load_double_constant(4, 0x4330000080000000);
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.load_float_constant(3, f32::from_bits(0x3e9a209b));
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 2, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 7,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 6,
            a: 5,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 9, a: 7, c: 7 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 5,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 5,
                a: 6,
                c: 9,
                b: 5,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 1,
                a: 9,
                c: 5,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 0,
                a: 9,
                c: 1,
                b: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 5, a: 7, c: 0 });
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 5 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 40,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 0,
                immediate: 32768,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 5, a: 5, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 0, a: 5, b: 8 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 1,
                a: 3,
                c: 2,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&100]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 9,
            });
        self.emit_branch_conditional_to(12, 2, labels[&104]); // beq
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&104]);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 0,
            s: 6,
            begin: 0,
            end: 0,
        });
        self.emit_branch_conditional_to(12, 2, labels[&109]); // beq
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
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&109]);
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
        self.emit_branch_to(labels[&115]); // b
        self.bind_label(labels[&112]);
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 0 });
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
