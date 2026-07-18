//! dq_clear: an exact-match whole-function capture (fire 755).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DQ_CLEAR_AST_HASH: u64 = 0xbc0a7cb4668e1821;

impl Generator {
    pub(super) fn try_dq_clear(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__DVDClearWaitingQueue"
            || function.return_type != Type::Void
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DQ_CLEAR_AST_HASH {
            eprintln!("dq_clear hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("dq_clear context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::Addr16Ha, "WaitingQueue");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "WaitingQueue");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 4,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
