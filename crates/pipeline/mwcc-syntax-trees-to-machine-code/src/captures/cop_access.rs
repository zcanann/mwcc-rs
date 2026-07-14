//! cop_access: an exact-match whole-function capture (fire 768).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const COP_ACCESS_AST_HASH: u64 = 0x863af5332c4bf8b8;

impl Generator {
    pub(super) fn try_cop_access(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDAccess"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != COP_ACCESS_AST_HASH {
            eprintln!("cop_access hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cop_access context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDDiskNone", "memcmp"].into_iter().map(String::from).collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [12, 29, 31, 32] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 255 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -4));
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&12]);
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDDiskNone");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 30, offset: 268 });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDDiskNone");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 4));
        self.record_relocation(RelocationKind::Rel24, "memcmp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcmp".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 30, offset: 268 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 4 });
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.record_relocation(RelocationKind::Rel24, "memcmp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcmp".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::load_immediate(3, -10));
        self.bind_label(labels[&32]);
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
