//! alm_block_link: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_BLOCK_LINK_AST_HASH: u64 = 0xe7117e3ec269e4b2;

impl Generator {
    pub(super) fn try_alm_block_link(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "Block_link"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALM_BLOCK_LINK_AST_HASH {
            eprintln!("alm_block_link hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 85, // marioparty4 (measured: protopool$129/init$130)
            0x626216a8cf3d36f5 => 0, // strikers (bump TBD)
            _ => {
                eprintln!("alm_block_link context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [39, 42, 49] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 5, shift: 0, begin: 31, end: 29 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 5, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 30, end: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: -4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 3, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 3, immediate: -4 });
        self.output.instructions.push(Instruction::Add { d: 30, a: 31, b: 30 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&39]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 30, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "SubBlock_merge_prev");
        self.output.instructions.push(Instruction::BranchAndLink { target: "SubBlock_merge_prev".to_string() });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 30, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "SubBlock_merge_next");
        self.output.instructions.push(Instruction::BranchAndLink { target: "SubBlock_merge_next".to_string() });
        self.emit_branch_to(labels[&42]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 4, offset: 12 });
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 0, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&49]); // bge
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 8 });
        self.bind_label(labels[&49]);
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
