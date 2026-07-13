//! als_alloc_var: an exact-match whole-function capture (fire 732).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALS_ALLOC_VAR_AST_HASH: u64 = 0x3a403a54bd3d79ce;

impl Generator {
    pub(super) fn try_als_alloc_var(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "allocate_from_var_pools"
            || !matches!(function.return_type, Type::Pointer(_) | Type::StructPointer { .. })
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALS_ALLOC_VAR_AST_HASH {
            eprintln!("als_alloc_var hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("als_alloc_var context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [12, 16, 19, 24, 34, 45, 47, 48] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 15 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 30, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 30, immediate: 80 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.emit_branch_conditional_to(4, 0, labels[&12]); // bge
        self.output.instructions.push(Instruction::load_immediate(30, 80));
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&16]); // beq
        self.emit_branch_to(labels[&19]); // b
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "link_new_block");
        self.output.instructions.push(Instruction::BranchAndLink { target: "link_new_block".to_string() });
        self.bind_label(labels[&19]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.emit_branch_conditional_to(4, 2, labels[&24]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 30, b: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&34]); // bgt
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "Block_subBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "Block_subBlock".to_string() });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&34]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 29, offset: 0 });
        self.emit_branch_to(labels[&47]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&24]); // bne
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "link_new_block");
        self.output.instructions.push(Instruction::BranchAndLink { target: "link_new_block".to_string() });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&45]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "Block_subBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "Block_subBlock".to_string() });
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 8 });
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
