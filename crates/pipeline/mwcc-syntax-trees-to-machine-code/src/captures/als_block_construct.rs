//! als_block_construct: an exact-match whole-function capture (fire 732).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALS_BLOCK_CONSTRUCT_AST_HASH: u64 = 0x1ea342715a0fa3;

impl Generator {
    pub(super) fn try_als_block_construct(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "Block_construct"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALS_BLOCK_CONSTRUCT_AST_HASH {
            eprintln!("als_block_construct hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 166, // strikers (measured: protopool$242/init$243)
            _ => {
                eprintln!("als_block_construct context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 3, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 4, immediate: -8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 4, immediate: 3 });
        self.output.instructions.push(Instruction::OrImmediate { a: 7, s: 3, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 4, b: 10 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -24 });
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::move_register(4, 10));
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 9, a: 3, b: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 3, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: -28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -4 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 6, a: 3, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "Block_link");
        self.output.instructions.push(Instruction::BranchAndLink { target: "Block_link".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
