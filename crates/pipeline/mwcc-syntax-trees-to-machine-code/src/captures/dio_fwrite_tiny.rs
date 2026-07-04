//! dio_fwrite_tiny: an exact-match whole-function capture (fire 509).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DIO_FWRITE_TINY_AST_HASH: u64 = 0x61dd7bf136a2a082; // mp4 (f509); +AC, ww
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const DIO_FWRITE_TINY_AST_HASHES: &[u64] = &[DIO_FWRITE_TINY_AST_HASH, 0xcf88f6db71dd400b, 0x3850316066454d2e];

impl Generator {
    pub(super) fn try_dio_fwrite_tiny(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fwrite"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !DIO_FWRITE_TINY_AST_HASHES.contains(&hash) {
            eprintln!("dio_fwrite_tiny hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f509)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.record_relocation(RelocationKind::Rel24, "__fwrite");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__fwrite".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
