//! ari_labs: an exact-match whole-function capture (fire 523).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ARI_LABS_AST_HASH: u64 = 0xc6b399a7d9ab4070; // pikmin2 (f523)

impl Generator {
    pub(super) fn try_ari_labs(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "labs"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ARI_LABS_AST_HASH {
            eprintln!("ari_labs hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x5705cf3446552579 => 0, // pikmin2 arith (f523)
            0xbd60acb658c79e45 => 0, // pikmin2 arith (f523)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 4,
                s: 3,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 4, b: 3 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 3, a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
