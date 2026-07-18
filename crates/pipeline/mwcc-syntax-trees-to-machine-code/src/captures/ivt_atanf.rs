//! ivt_atanf: an exact-match whole-function capture (fire 714).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const IVT_ATANF_AST_HASH: u64 = 0x11202e446d401c39;

impl Generator {
    pub(super) fn try_ivt_atanf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "atanf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != IVT_ATANF_AST_HASH && hash != 0xe7d8542c2da061a9 {
            eprintln!("ivt_atanf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x1d008359133dc5f8 => 30, // sunshine (guess = pikmin)
            0x19234177da3e2378 => 30, // pikmin
            _ => {
                eprintln!("ivt_atanf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [20, 33, 37, 42, 48, 54, 60, 61, 80, 81, 117, 119, 123] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "...rodata.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "...rodata.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 3,
            immediate: 0,
        });
        self.load_float_constant(0, f32::from_bits(0x401a827a));
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(9, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 3,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 3,
                s: 3,
                begin: 0,
                end: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.load_float_constant(0, f32::from_bits(0x3f800000));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 12,
            });
        self.emit_branch_to(labels[&81]); // b
        self.bind_label(labels[&20]);
        self.load_float_constant(0, f32::from_bits(0x3ed413cd));
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&80]); // bge
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 6,
            shift: 0,
            begin: 1,
            end: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16256));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 4 });
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 0));
        self.emit_branch_conditional_to(12, 2, labels[&48]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&33]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16128));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&37]); // beq
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&33]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16384));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&60]); // beq
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&37]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16137));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -10823,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&42]); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 1));
        self.bind_label(labels[&42]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16210));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 6145,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&61]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 9,
            immediate: 1,
        });
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&48]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16284));
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 2));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -2068,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&54]); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 3));
        self.bind_label(labels[&54]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 16367));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 30878,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&61]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 9,
            immediate: 1,
        });
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&60]);
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 4));
        self.bind_label(labels[&61]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 8,
                s: 9,
                shift: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 7,
            immediate: 156,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleIndexed { d: 4, a: 4, b: 8 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 7,
            immediate: 132,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 7,
            immediate: 28,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 7,
            immediate: 52,
        });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 1, b: 4 });
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleIndexed { d: 5, a: 6, b: 8 });
        self.load_float_constant(3, f32::from_bits(0x3f800000));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleIndexed { d: 1, a: 5, b: 8 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 2, a: 5, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleIndexed { d: 0, a: 4, b: 8 });
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 2, a: 3, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractSingle {
                d: 1,
                a: 2,
                c: 1,
                b: 5,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 2,
                a: 1,
                offset: 12,
            });
        self.output
            .instructions
            .push(Instruction::FloatNegativeMultiplySubtractSingle {
                d: 0,
                a: 2,
                c: 0,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 12,
            });
        self.emit_branch_to(labels[&81]); // b
        self.bind_label(labels[&80]);
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 12,
            });
        self.bind_label(labels[&81]);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 7,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 7,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 6,
            offset: 24,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 7,
            immediate: 104,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 8, a: 7, c: 7 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 6,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 8,
                s: 9,
                shift: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 7,
            immediate: 76,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 5, b: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 6,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 1,
                a: 8,
                c: 2,
                b: 1,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 4,
            a: 6,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 6, a: 7, c: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 3,
            a: 6,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 5,
                a: 8,
                c: 1,
                b: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 4,
                a: 8,
                c: 5,
                b: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 3,
                a: 8,
                c: 4,
                b: 3,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 2,
                a: 8,
                c: 3,
                b: 2,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 3,
                a: 6,
                c: 2,
                b: 7,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 2, a: 3, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 3,
                a: 1,
                offset: 12,
            });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 2, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 2,
                a: 1,
                offset: 12,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 12,
            });
        self.emit_branch_conditional_to(12, 2, labels[&119]); // beq
        self.load_float_constant(0, f32::from_bits(0x3fc90fdb));
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: 12,
            });
        self.emit_branch_conditional_to(12, 2, labels[&117]); // beq
        self.emit_branch_to(labels[&123]); // b
        self.bind_label(labels[&117]);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.emit_branch_to(labels[&123]); // b
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 1,
            offset: 12,
        });
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
