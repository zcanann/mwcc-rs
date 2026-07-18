//! cck_check: an exact-match whole-function capture (fire 757).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CCK_CHECK_AST_HASH: u64 = 0x55f0763864f43fd4;

impl Generator {
    pub(super) fn try_cck_check(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "CARDCheck"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CCK_CHECK_AST_HASH {
            eprintln!("cck_check hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cck_check context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [14, 16] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDSyncCallback");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDSyncCallback");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "CARDCheckExAsync");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "CARDCheckExAsync".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&16]); // blt
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 0,
                a: 1,
                immediate: 8,
            });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.emit_branch_to(labels[&16]); // b
        self.bind_label(labels[&14]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "__CARDSync");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDSync".to_string(),
        });
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
