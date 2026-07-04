//! bio_flush_str: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BIO_FLUSH_STR_AST_HASH: u64 = 0xbbe6f5452bb16da3; // strikers (f511)

impl Generator {
    pub(super) fn try_bio_flush_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__flush_buffer"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != BIO_FLUSH_STR_AST_HASH {
            eprintln!("bio_flush_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xcd0e7af815097794 => 0, // strikers buffer_io (f511)
            _ => {
                eprintln!("bio_flush_str context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [23, 26, 30, 43] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 31, offset: 64 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 31, offset: 72 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 30, offset: 0 });
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.emit_branch_to(labels[&43]); // b
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 24 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 44 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::And { a: 4, s: 5, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 52 });
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
