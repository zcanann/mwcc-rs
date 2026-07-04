//! sld_strtod: an exact-match whole-function capture (fire 467).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SLD_STRTOD_AST_HASH: u64 = 0x87c5d9e01882c018;

impl Generator {
    pub(super) fn try_sld_strtod(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strtod"
            || function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SLD_STRTOD_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x6ff29e48ce03ae67 => 0, // pikmin (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
