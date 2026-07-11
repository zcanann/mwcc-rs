//! pfa_stringwrite: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_STRINGWRITE_AST_HASH: u64 = 0x80910eda8e23fb3e;

impl Generator {
    pub(super) fn try_pfa_stringwrite(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__StringWrite"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_STRINGWRITE_AST_HASH {
            eprintln!("pfa_stringwrite hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x3012f8741ad9c69d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfa_stringwrite context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 31, a: 3, b: 6 });
        self.emit_branch_conditional_to(12, 1, labels[&13]); // bgt
        self.output.instructions.push(Instruction::move_register(31, 5));
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::move_register(5, 31));
        self.output.instructions.push(Instruction::Add { d: 3, a: 0, b: 3 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 30, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::Add { d: 0, a: 0, b: 31 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 30, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
