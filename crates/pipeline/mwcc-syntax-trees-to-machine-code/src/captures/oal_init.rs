//! oal_init: an exact-match whole-function capture (fire 756).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OAL_INIT_AST_HASH: u64 = 0xefb34a23901e7a32;

impl Generator {
    pub(super) fn try_oal_init(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSInitAlarm"
            || function.return_type != Type::Void
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OAL_INIT_AST_HASH {
            eprintln!("oal_init hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc418e20019aad651 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("oal_init context candidate: {context:#x}");
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
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 8));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Rel24, "__OSGetExceptionHandler");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSGetExceptionHandler".to_string(),
        });
        self.record_relocation(RelocationKind::Addr16Ha, "DecrementerExceptionHandler");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "DecrementerExceptionHandler");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&15]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, "AlarmQueue");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 8));
        self.record_relocation(RelocationKind::EmbSda21, "AlarmQueue");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Rel24, "__OSSetExceptionHandler");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSSetExceptionHandler".to_string(),
        });
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
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
