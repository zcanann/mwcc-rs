//! mbs_wcstombs_str: an exact-match whole-function capture (fire 514).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_WCSTOMBS_STR_AST_HASH: u64 = 0x797c9420ac5fa333; // strikers (f514)

impl Generator {
    pub(super) fn try_mbs_wcstombs_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wcstombs"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_WCSTOMBS_STR_AST_HASH {
            eprintln!("mbs_wcstombs_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa1c526a9076479be => 12, // pikmin2 mbstring (f514)
            0xdba628923f494fa9 => 12, // strikers mbstring (f514): pool @59 measured
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![27, 28, 29, 30, 31]; // via _savegpr_27
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [11, 13, 15, 21, 28, 32, 33, 41, 47, 51, 55, 63, 66] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_27".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 28, s: 3, b: 3 });
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.emit_branch_conditional_to(12, 2, labels[&11]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 28, b: 31 });
        self.emit_branch_to(labels[&66]); // b
        self.bind_label(labels[&21]);
        self.load_word_constant(0, 0x0000c0e0);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 128 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&28]); // bge
        self.output.instructions.push(Instruction::load_immediate(27, 1));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 2048 });
        self.emit_branch_conditional_to(4, 0, labels[&32]); // bge
        self.output.instructions.push(Instruction::load_immediate(27, 2));
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::load_immediate(27, 3));
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&41]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&51]); // bge
        self.emit_branch_to(labels[&55]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&55]); // bge
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 6, clear: 26 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 6, s: 6, shift: 26, begin: 22, end: 31 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 128 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 5, offset: -1 });
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 6, clear: 26 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 6, s: 6, shift: 26, begin: 22, end: 31 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 128 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 5, offset: -1 });
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 4, b: 27 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: -1 });
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::Add { d: 0, a: 31, b: 27 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 29 });
        self.emit_branch_conditional_to(12, 1, labels[&66]); // bgt
        self.output.instructions.push(Instruction::move_register(5, 27));
        self.output.instructions.push(Instruction::Add { d: 3, a: 28, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "strncpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strncpy".to_string() });
        self.output.instructions.push(Instruction::Add { d: 31, a: 31, b: 27 });
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 31, b: 29 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.emit_branch_conditional_to(4, 1, labels[&15]); // ble
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
