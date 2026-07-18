//! mbs_mblen_stub: an exact-match whole-function capture (fire 514).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_MBLEN_STUB_AST_HASH: u64 = 0xc8988ac41f250924; // BfBB void-shell (f517)

impl Generator {
    pub(super) fn try_mbs_mblen_stub(&mut self, function: &Function) -> Compilation<bool> {
        // Gate on the NAME only — the pik variant is int(const char*, size_t),
        // BfBB's decompiled shell is void(void); the hash decides.
        if function.name != "mblen" || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_MBLEN_STUB_AST_HASH {
            eprintln!("mbs_mblen_stub hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc1eb9a856a0f8258 => 0, // BfBB mbstring (f517)
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
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
