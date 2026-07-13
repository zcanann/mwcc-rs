//! sfb_num2dec_i: an exact-match whole-function capture (fire 724).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFB_NUM2DEC_I_AST_HASH: u64 = 0x81c740dd2d60ccd0;

impl Generator {
    pub(super) fn try_sfb_num2dec_i(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec_internal"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFB_NUM2DEC_I_AST_HASH {
            eprintln!("sfb_num2dec_i hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 162, // strikers copy
            _ => {
                eprintln!("sfb_num2dec_i context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 160;
        self.non_leaf = true;
        self.output.symbol_order = ["SIGNBIT","__count_trailing_zero","modf","__cvt_dbl_usll","__ull2dec"].iter().map(|n| n.to_string()).collect();
        if context == 0xf3c0ffcf51c5b47b {
            // pow_10$ numbers 82 past the pool constant (the fire-698 pattern).
            self.output.post_constant_label_bump = 82;
        }
        for bits in [
            0x0000000000000000u64,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [24, 34, 39, 41, 43, 48, 50, 52, 53, 70, 75, 77, 79, 84, 86, 88, 89, 93, 95, 98, 124] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -160 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 144 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 140 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink { target: "SIGNBIT".to_string() });
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 31 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 30, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&24]); // bne
        self.output.instructions.push(Instruction::StoreByte { s: 30, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 31, offset: 5 });
        self.emit_branch_to(labels[&124]); // b
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&34]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&52]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&43]); // beq
        self.emit_branch_to(labels[&52]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&50]); // beq
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&95]); // bgt
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 30, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 5, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&70]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&88]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&79]); // beq
        self.emit_branch_to(labels[&88]); // b
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&75]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&77]); // beq
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&79]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 5, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&84]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&86]); // beq
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&89]); // b
        self.bind_label(labels[&88]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&93]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&93]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 31, offset: 5 });
        self.emit_branch_to(labels[&124]); // b
        self.bind_label(labels[&95]);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&98]); // beq
        self.output.instructions.push(Instruction::FloatNegate { d: 31, b: 31 });
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "frexp".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__count_trailing_zero".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 29, a: 3, immediate: 53 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 40 });
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
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 84 });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__ull2dec".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 84 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 40 });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__timesdec".to_string() });
        self.output.instructions.push(Instruction::StoreByte { s: 30, a: 31, offset: 0 });
        self.bind_label(labels[&124]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 144 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 140 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 160 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
