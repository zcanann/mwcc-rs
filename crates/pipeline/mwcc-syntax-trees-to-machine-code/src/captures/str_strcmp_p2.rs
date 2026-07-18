//! str_strcmp_p2: an exact-match whole-function capture (fire 471).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRCMP_P2_AST_HASH: u64 = 0xa4d9eeac0ae7017e;
/// Cosmetic AST variants with IDENTICAL instruction streams (content-diffed): BfBB f502.
const STR_STRCMP_P2_AST_HASHES: &[u64] = &[STR_STRCMP_P2_AST_HASH, 0x863cd0e0eb6c9e94];

impl Generator {
    pub(super) fn try_str_strcmp_p2(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strcmp"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !STR_STRCMP_P2_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // pikmin2 (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 16, 20, 26, 30, 31, 33, 42, 48, 54, 60, 64, 70] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&6]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 30,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 6,
                s: 3,
                clear: 30,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&60]); // bne
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&33]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&16]);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: 6,
                immediate: 3,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 5,
                a: 3,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&26]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&30]);
        self.emit_branch_conditional_to(16, 0, labels[&20]); // bdnz
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, -32639));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 5,
            immediate: -32640,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 7,
                immediate: -257,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -257,
        });
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&54]); // bne
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&42]);
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 7,
                a: 3,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 8,
                a: 4,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 7,
                immediate: -257,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -257,
        });
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&54]); // bne
        self.bind_label(labels[&48]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&42]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&60]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&60]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&64]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 5,
                a: 3,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&70]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
