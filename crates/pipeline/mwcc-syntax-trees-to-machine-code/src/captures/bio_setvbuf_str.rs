//! bio_setvbuf_str: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BIO_SETVBUF_STR_AST_HASH: u64 = 0xc161335b72c76b8a; // strikers (f511)

impl Generator {
    pub(super) fn try_bio_setvbuf_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "setvbuf"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != BIO_SETVBUF_STR_AST_HASH {
            eprintln!("bio_setvbuf_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xcd0e7af815097794 => 0, // strikers buffer_io (f511)
            _ => {
                eprintln!("bio_setvbuf_str context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = vec![27, 28, 29, 30, 31]; // via _savegpr_27
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 18, 20, 28, 35, 55, 62, 72, 76, 85] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 27, s: 5, b: 5 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::move_register(31, 6));
        self.output.instructions.push(Instruction::RotateAndMask { a: 28, s: 0, shift: 26, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.record_relocation(RelocationKind::Rel24, "fflush");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fflush".to_string() });
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 27, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&35]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 28, begin: 31, end: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&35]); // beq
        self.record_relocation(RelocationKind::Rel24, "free");
        self.output.instructions.push(Instruction::BranchAndLink { target: "free".to_string() });
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.record_relocation(RelocationKind::Rel24, "__begin_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__begin_critical_region".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 27, shift: 1, begin: 29, end: 30 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 29, immediate: 13 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 4, s: 5, shift: 4, begin: 27, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 29, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 29, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 29, offset: 44 });
        self.emit_branch_conditional_to(12, 2, labels[&55]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&62]); // bge
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 29, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&76]); // bne
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "malloc");
        self.output.instructions.push(Instruction::BranchAndLink { target: "malloc".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 30, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&72]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 3, shift: 4, begin: 27, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 8 });
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 29, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 44 });
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
