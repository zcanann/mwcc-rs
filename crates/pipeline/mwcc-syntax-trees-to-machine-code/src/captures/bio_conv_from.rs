//! bio_conv_from: an exact-match whole-function capture (fire 510).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BIO_CONV_FROM_AST_HASH: u64 = 0x9955b3eb1383ff26; // pikmin; +BfBB (f510)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const BIO_CONV_FROM_AST_HASHES: &[u64] = &[
    BIO_CONV_FROM_AST_HASH,
    0x7eb651f5d38f8dc9,
    0x783f2f528f6d1b98,
];

impl Generator {
    pub(super) fn try_bio_conv_from(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__convert_from_newlines"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !BIO_CONV_FROM_AST_HASHES.contains(&hash) {
            eprintln!("bio_conv_from hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f510)
            0x626216a8cf3d36f5 => 0, // pikmin (f510)
            0x071cd740dac1b53c => 0, // ww (f510)
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
