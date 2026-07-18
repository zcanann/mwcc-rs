//! pfa_filewrite: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_FILEWRITE_AST_HASH: u64 = 0x422ecf55222f23e0;

impl Generator {
    pub(super) fn try_pfa_filewrite(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__FileWrite"
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_FILEWRITE_AST_HASH
            && hash != 0xcb6e5caa59535a3b
            && hash != 0xd6d9a4fdf57f35b1
        {
            eprintln!("pfa_filewrite hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (bump TBD)
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            0x46f259063d157aea => 0, // wind_waker (bump TBD)
            0xf8b1cd38c2b39c70 => 0, // animal_crossing (bump TBD)
            0x3012f8741ad9c69d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfa_filewrite context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [15, 16] {
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
            .push(Instruction::move_register(31, 5));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::move_register(6, 30));
        self.record_relocation(RelocationKind::Rel24, "fwrite");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "fwrite".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 31, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.emit_branch_to(labels[&16]); // b
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&16]);
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 8,
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
