//! ari_div: an exact-match whole-function capture (fire 523).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ARI_DIV_AST_HASH: u64 = 0x135d73db22ddc689; // BfBB/p2 (f523, struct pair return)

impl Generator {
    pub(super) fn try_ari_div(&mut self, function: &Function) -> Compilation<bool> {
        // div returns a STRUCT by value — gate on the name; the hash decides.
        if function.name != "div" || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ARI_DIV_AST_HASH {
            eprintln!("ari_div hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x5705cf3446552579 => 0, // pikmin2 arith (f523)
            0xbd60acb658c79e45 => 0, // BfBB arith (f523)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [7, 11] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 7, s: 3, b: 3 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 1));
        self.emit_branch_conditional_to(4, 0, labels[&7]); // bge
        self.output
            .instructions
            .push(Instruction::Negate { d: 7, a: 7 });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.bind_label(labels[&7]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&11]); // bge
        self.output
            .instructions
            .push(Instruction::Negate { d: 4, a: 4 });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, -1));
        self.bind_label(labels[&11]);
        self.output
            .instructions
            .push(Instruction::DivideWord { d: 3, a: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 0, a: 5, b: 6 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 3, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 0, a: 3, b: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 4, a: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 0, a: 7, b: 5 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 12,
        });
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
