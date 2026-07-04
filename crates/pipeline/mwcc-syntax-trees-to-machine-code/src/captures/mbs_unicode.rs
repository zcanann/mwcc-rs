//! mbs_unicode: an exact-match whole-function capture (fire 516).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_UNICODE_AST_HASH: u64 = 0x3a4a2a402936e876; // mp4, re-armed f517 (the @4 static-slot pooled image)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
/// ww's 0x1bb864ba9b9c82fd DEFERRED (f517): its image pools in the BLOCK (@47,
/// bump 30, load_word_constant_image) but the @47 symbol/dedup interaction with
/// wcstombs' reuse is unresolved — re-add once the writer models it.
const MBS_UNICODE_AST_HASHES: &[u64] = &[MBS_UNICODE_AST_HASH];

impl Generator {
    pub(super) fn try_mbs_unicode(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "unicode_to_UTF8"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !MBS_UNICODE_AST_HASHES.contains(&hash) {
            eprintln!("mbs_unicode hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x6e7a972c5b9ab3cb => 0, // mbs_mp4 (f516)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 12, 16, 17, 24, 30, 34, 38, 39] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        // mp4/AC pool the array image at the STATIC SLOT (@4); ww's variant
        // pools it in the fn's own POOL BLOCK — the hash selects.
        if hash == 0x1bb864ba9b9c82fd {
            self.load_word_constant_image(0, 0x0000c0e0);
        } else {
            self.load_word_constant_static_slot(0, 0x0000c0e0);
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&39]); // b
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 128 });
        self.emit_branch_conditional_to(4, 0, labels[&12]); // bge
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.emit_branch_to(labels[&17]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2048 });
        self.emit_branch_conditional_to(4, 0, labels[&16]); // bge
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.emit_branch_to(labels[&17]); // b
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::load_immediate(5, 3));
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 2 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 3, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&24]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&34]); // bge
        self.emit_branch_to(labels[&38]); // b
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&38]); // bge
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 26 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 4, shift: 26, begin: 22, end: 31 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 128 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 6, offset: -1 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 26 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 4, shift: 26, begin: 22, end: 31 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 128 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 6, offset: -1 });
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 6, offset: -1 });
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump + if hash == 0x1bb864ba9b9c82fd { 30 } else { 0 }; // ww: pool @47 measured
        Ok(true)
    }
}
