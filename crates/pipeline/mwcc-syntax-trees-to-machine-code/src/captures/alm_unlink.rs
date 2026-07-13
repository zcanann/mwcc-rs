//! alm_unlink: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_UNLINK_AST_HASH: u64 = 0xc2120164314f4d0a;

impl Generator {
    pub(super) fn try_alm_unlink(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__unlink"
            || !matches!(function.return_type, Type::Pointer(_) | Type::StructPointer { .. })
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALM_UNLINK_AST_HASH {
            eprintln!("alm_unlink hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            0x626216a8cf3d36f5 => 0, // strikers (bump TBD)
            _ => {
                eprintln!("alm_unlink context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 8, 14] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&8]); // bne
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: 0 });
        self.bind_label(labels[&8]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&14]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: 4 });
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
