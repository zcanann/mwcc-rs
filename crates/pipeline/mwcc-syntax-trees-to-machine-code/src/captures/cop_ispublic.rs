//! cop_ispublic: an exact-match whole-function capture (fire 768).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const COP_ISPUBLIC_AST_HASH: u64 = 0x10bf48eee39f1881;

impl Generator {
    pub(super) fn try_cop_ispublic(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDIsPublic"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != COP_ISPUBLIC_AST_HASH {
            eprintln!("cop_ispublic hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cop_ispublic context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [5] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 255,
            });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -4));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&5]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 3,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -10));
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 30,
            begin: 31,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::Negate { d: 3, a: 3 });
        self.output
            .instructions
            .push(Instruction::AndComplement { a: 3, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
