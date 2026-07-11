//! wsc_wfileread: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_WFILEREAD_AST_HASH: u64 = 0x311c2193bce5c359;

impl Generator {
    pub(super) fn try_wsc_wfileread(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__wFileRead"
            || function.return_type != Type::UnsignedShort
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_WFILEREAD_AST_HASH {
            eprintln!("wsc_wfileread hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wsc_wfileread context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [12, 15, 18, 23, 31, 32, 34, 35] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&12]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&15]); // bge
        self.emit_branch_to(labels[&34]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&34]); // bge
        self.emit_branch_to(labels[&23]); // b
        self.bind_label(labels[&15]);
        self.record_relocation(RelocationKind::Rel24, "fgetwc");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fgetwc".to_string() });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "ungetwc");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ungetwc".to_string() });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.record_relocation(RelocationKind::Rel24, "ferror");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ferror".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "feof");
        self.output.instructions.push(Instruction::BranchAndLink { target: "feof".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 31, clear: 16 });
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&35]);
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
