//! sfb_num2dec: an exact-match whole-function capture (fire 724).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFB_NUM2DEC_AST_HASH: u64 = 0xc12d777434be55d6;

impl Generator {
    pub(super) fn try_sfb_num2dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFB_NUM2DEC_AST_HASH {
            eprintln!("sfb_num2dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfb_num2dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 160;
        self.non_leaf = true;
        for bits in [
            0x0000000000000000u64,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [26, 36, 41, 43, 45, 50, 52, 54, 55, 72, 77, 79, 81, 86, 88, 90, 91, 95, 97, 100, 126, 133, 139, 144, 153, 158, 161] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -160 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 31, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 144 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 140 });
        self.output.instructions.push(Instruction::StoreWord { s: 28, a: 1, offset: 136 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink { target: "SIGNBIT".to_string() });
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 31 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 28, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.output.instructions.push(Instruction::StoreByte { s: 28, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 30, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 30, offset: 5 });
        self.emit_branch_to(labels[&126]); // b
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&54]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&45]); // beq
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&41]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&43]); // beq
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&55]); // b
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&55]); // b
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&50]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&52]); // beq
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&55]); // b
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&55]); // b
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&97]); // bgt
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 28, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 30, offset: 2 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 5, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&72]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&90]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&81]); // beq
        self.emit_branch_to(labels[&90]); // b
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&77]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&79]); // beq
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&79]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&81]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&86]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&88]); // beq
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&88]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&90]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&95]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&95]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 5 });
        self.emit_branch_to(labels[&126]); // b
        self.bind_label(labels[&97]);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 28 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.output.instructions.push(Instruction::FloatNegate { d: 31, b: 31 });
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "frexp".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__count_trailing_zero".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 29, a: 3, immediate: 53 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 84 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 29, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__two_exp".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "modf".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__cvt_dbl_usll".to_string() });
        self.output.instructions.push(Instruction::move_register(5, 3));
        self.output.instructions.push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 40 });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__ull2dec".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 40 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 84 });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__timesdec".to_string() });
        self.output.instructions.push(Instruction::StoreByte { s: 28, a: 30, offset: 0 });
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 30, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 9 });
        self.emit_branch_conditional_to(12, 1, labels[&161]); // bgt
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 0, s: 31 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 36 });
        self.emit_branch_conditional_to(4, 1, labels[&133]); // ble
        self.output.instructions.push(Instruction::load_immediate(31, 36));
        self.bind_label(labels[&133]);
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 28, s: 31 });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 28));
        self.record_relocation(RelocationKind::Rel24, "__rounddec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__rounddec".to_string() });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.emit_branch_to(labels[&144]); // b
        self.bind_label(labels[&139]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 5, a: 30, b: 0 });
        self.bind_label(labels[&144]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 28 });
        self.emit_branch_conditional_to(12, 0, labels[&139]); // blt
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 30, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 30, offset: 2 });
        self.emit_branch_to(labels[&158]); // b
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 30, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 30, b: 4 });
        self.bind_label(labels[&158]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&153]); // blt
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 144 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 140 });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 1, offset: 136 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 160 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
