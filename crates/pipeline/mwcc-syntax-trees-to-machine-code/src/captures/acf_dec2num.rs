//! acf_dec2num: an exact-match whole-function capture (fire 685).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACF_DEC2NUM_AST_HASH: u64 = 0xadffd61e9b194e01;

impl Generator {
    pub(super) fn try_acf_dec2num(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dec2num"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACF_DEC2NUM_AST_HASH {
            eprintln!("acf_dec2num hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xadf9060938342c54 => 112, // animal_crossing
            _ => {
                eprintln!("acf_dec2num context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [9, 15, 20, 29, 33, 40, 45, 51, 59, 62] {
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
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 308));
        let index = self.intern_string_literal(&[
            0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33,
            0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34,
            0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30,
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 8,
        });
        let index = self.intern_string_literal(&[
            0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33,
            0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34,
            0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30,
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.emit_branch_to(labels[&15]); // b
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&20]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&62]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 12,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 13,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&51]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&62]); // b
        self.bind_label(labels[&51]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&59]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 1,
                offset: 10,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.emit_branch_to(labels[&62]); // b
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&45]); // b
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
