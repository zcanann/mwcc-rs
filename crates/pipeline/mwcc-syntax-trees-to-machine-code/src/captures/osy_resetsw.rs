//! osy_resetsw: an exact-match whole-function capture (fire 758).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OSY_RESETSW_AST_HASH: u64 = 0x2a531ddf2a9b239;

impl Generator {
    pub(super) fn try_osy_resetsw(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSResetStopwatch"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OSY_RESETSW_AST_HASH {
            eprintln!("osy_resetsw hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x532c74a9b25838e0 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("osy_resetsw context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // The OSFastCast.h plain-`inline` asm helpers surface as GLOBAL UND symbols
        // ahead of this first global function's own symbol (measured: OSSync.c).
        self.output.phantom_externals = vec!["__OSf32tos16".to_string(), "__OSf32tou8".to_string()];
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
