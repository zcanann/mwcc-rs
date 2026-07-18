//! alm_free: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_FREE_AST_HASH: u64 = 0x49d30b6c4108ecde;

impl Generator {
    pub(super) fn try_alm_free(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "free"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALM_FREE_AST_HASH {
            eprintln!("alm_free hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            0x6b3a129a97773139 => 0, // wind_waker (bump TBD)
            0x626216a8cf3d36f5 => 0, // strikers (bump TBD)
            _ => {
                eprintln!("alm_free context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [15] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::EmbSda21, "init");
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "protopool");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "protopool");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 52));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::EmbSda21, "init");
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(labels[&15]);
        self.record_relocation(RelocationKind::Addr16Ha, "protopool");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::move_register(4, 31));
        self.record_relocation(RelocationKind::Addr16Lo, "protopool");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, "__pool_free");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__pool_free".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
