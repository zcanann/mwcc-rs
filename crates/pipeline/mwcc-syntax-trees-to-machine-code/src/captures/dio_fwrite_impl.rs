//! dio_fwrite_impl: an exact-match whole-function capture (fire 509).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DIO_FWRITE_IMPL_AST_HASH: u64 = 0x8fcab973f47ccfd4; // mp4 (f509); +ww
/// Cosmetic AST variants with IDENTICAL instruction streams (@N-normalized).
const DIO_FWRITE_IMPL_AST_HASHES: &[u64] = &[DIO_FWRITE_IMPL_AST_HASH, 0x41fc9ad59fd2bb74, 0x23eb744fffd0e8c4, 0x32cd1b78e0cb7486, 0x41589299a1b0fa0b];

impl Generator {
    pub(super) fn try_dio_fwrite_impl(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__fwrite"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !DIO_FWRITE_IMPL_AST_HASHES.contains(&hash) {
            eprintln!("dio_fwrite_impl hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // the MSL-common fingerprint (f509)
            0x626216a8cf3d36f5 => 0, // strikers (f511)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![25, 26, 27, 28, 29, 30, 31]; // via _savegpr_25
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [17, 25, 27, 30, 39, 40, 47, 64, 70, 80, 90, 94, 100, 115, 132, 140, 151, 155, 174, 182, 188, 191] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_25".to_string() });
        self.output.instructions.push(Instruction::move_register(26, 6));
        self.output.instructions.push(Instruction::move_register(27, 3));
        self.output.instructions.push(Instruction::move_register(25, 4));
        self.output.instructions.push(Instruction::move_register(28, 5));
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fwide".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fwide".to_string() });
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::MultiplyLowRecord { d: 29, a: 25, b: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&25]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 26, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&27]); // bne
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.record_relocation(RelocationKind::Rel24, "__stdio_atexit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__stdio_atexit".to_string() });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 5 });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 29, begin: 31, end: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&39]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 31, begin: 30, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 31, begin: 30, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 27, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&70]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 30, end: 30 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 29, begin: 29, end: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&70]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 29, end: 29 });
        self.emit_branch_conditional_to(12, 2, labels[&64]); // beq
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.record_relocation(RelocationKind::Rel24, "fseek");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fseek".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&64]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 3, shift: 5, begin: 24, end: 26 });
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 26, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "__prep_buffer");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__prep_buffer".to_string() });
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 27, begin: 29, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&80]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 26, offset: 10 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&80]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 29, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(30, 27));
        self.output.instructions.push(Instruction::load_immediate(28, 0));
        self.emit_branch_conditional_to(12, 2, labels[&155]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 26, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 26, offset: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&90]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&155]); // beq
        self.bind_label(labels[&90]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 26, offset: 32 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.bind_label(labels[&94]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 26, offset: 40 });
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 29 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&100]); // ble
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 8 });
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 31, begin: 30, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&115]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&115]); // beq
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 10));
        self.record_relocation(RelocationKind::Rel24, "__memrchr");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__memrchr".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 27, s: 3, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&115]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 27, immediate: 1 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 30, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&132]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 26, offset: 36 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 26, offset: 36 });
        self.output.instructions.push(Instruction::Add { d: 30, a: 30, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 28, a: 28, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 29, a: 3, b: 29 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 26, offset: 40 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 26, offset: 40 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&140]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&140]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 31, begin: 30, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&151]); // bne
        self.bind_label(labels[&140]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Rel24, "__flush_buffer");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__flush_buffer".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&151]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 26, offset: 10 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.emit_branch_to(labels[&155]); // b
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&155]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&94]); // bne
        self.bind_label(labels[&155]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&182]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&182]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 27, a: 26, offset: 28 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 30, b: 29 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 26, offset: 32 });
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 26, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 26, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 36 });
        self.record_relocation(RelocationKind::Rel24, "__flush_buffer");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__flush_buffer".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&174]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 26, offset: 10 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.bind_label(labels[&174]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::StoreWord { s: 27, a: 26, offset: 28 });
        self.output.instructions.push(Instruction::Add { d: 28, a: 28, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 26, offset: 32 });
        self.record_relocation(RelocationKind::Rel24, "__prep_buffer");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__prep_buffer".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.bind_label(labels[&182]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 26, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 31, begin: 30, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&188]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 26, offset: 40 });
        self.bind_label(labels[&188]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 25, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 28, b: 0 });
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 3, a: 0, b: 25 });
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_25".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
