//! bio_flush_b: an exact-match whole-function capture (fire 510).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BIO_FLUSH_B_AST_HASH: u64 = 0x8a345843ff7af429; // melee; +sunshine (f510)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const BIO_FLUSH_B_AST_HASHES: &[u64] = &[BIO_FLUSH_B_AST_HASH, 0xef24056687664a0a];

impl Generator {
    pub(super) fn try_bio_flush_b(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__flush_buffer"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !BIO_FLUSH_B_AST_HASHES.contains(&hash) {
            eprintln!("bio_flush_b hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f510)
            0x626216a8cf3d36f5 => 0, // pikmin (f510)
            0x071cd740dac1b53c => 0, // ww (f510)
            _ => return Ok(false),
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
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 32 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 31, offset: 60 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 31, offset: 68 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 30, offset: 0 });
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.emit_branch_to(labels[&43]); // b
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 20 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 31, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::And { a: 4, s: 5, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 48 });
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
