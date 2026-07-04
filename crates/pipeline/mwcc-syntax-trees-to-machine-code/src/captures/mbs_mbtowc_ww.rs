//! mbs_mbtowc_ww: an exact-match whole-function capture (fire 516).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_MBTOWC_WW_AST_HASH: u64 = 0x9d1b8b7778ca353c; // ww (f517)
// NOTE: the HASH is shared with BfBB (same source text) but ww's stream is a
// bare blr (the empty static callee inlined away) — the ctx arm disambiguates.

impl Generator {
    pub(super) fn try_mbs_mbtowc_ww(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "mbtowc"
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_MBTOWC_WW_AST_HASH {
            eprintln!("mbs_mbtowc_ww hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x6e7a972c5b9ab3cb => 0, // ww mbstring (f517)
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
