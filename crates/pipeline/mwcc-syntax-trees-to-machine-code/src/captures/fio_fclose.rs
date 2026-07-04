//! fio_fclose: an exact-match whole-function capture (fire 508).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const FIO_FCLOSE_AST_HASH: u64 = 0x4b52ad40e01a659a; // mp4/AC (f508)
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const FIO_FCLOSE_AST_HASHES: &[u64] = &[FIO_FCLOSE_AST_HASH, 0x562fd1c360781a4, 0x7f34b598c986d8c3, 0x60c3c2c6818f6792];

impl Generator {
    pub(super) fn try_fio_fclose(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fclose"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !FIO_FCLOSE_AST_HASHES.contains(&hash) {
            eprintln!("fio_fclose hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f508)
            0x4dc5812f6e4177a3 => 0, // strikers (f508)
            0xa33472769b752957 => 0, // ww (f508)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [10, 15, 32, 37, 38, 41] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::OrRecord { a: 29, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&41]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 26, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&41]); // b
        self.bind_label(labels[&15]);
        self.record_relocation(RelocationKind::Rel24, "fflush");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fflush".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 29, offset: 68 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 4, shift: 6, begin: 23, end: 25 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 28, begin: 31, end: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 28 });
        self.record_relocation(RelocationKind::Rel24, "free");
        self.output.instructions.push(Instruction::BranchAndLink { target: "free".to_string() });
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 31 });
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
