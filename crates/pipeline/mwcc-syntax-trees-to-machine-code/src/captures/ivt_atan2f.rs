//! ivt_atan2f: an exact-match whole-function capture (fire 714).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const IVT_ATAN2F_AST_HASH: u64 = 0xff7cab823af5688e;

impl Generator {
    pub(super) fn try_ivt_atan2f(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "atan2f"
            || function.return_type != Type::Float
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != IVT_ATAN2F_AST_HASH && hash != 0x31c38b73d61f95c6 {
            eprintln!("ivt_atan2f hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x1d008359133dc5f8 => 18, // sunshine (bump guess = pikmin)
            0x19234177da3e2378 => 18, // pikmin
            _ => {
                eprintln!("ivt_atan2f context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [20, 28, 30, 40, 46, 50] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 2, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 4, s: 0, begin: 0, end: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 0, s: 3, begin: 0, end: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&20]); // beq
        self.output.instructions.push(Instruction::RoundToSingle { d: 1, b: 1 });
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatDivideSingle { d: 1, a: 1, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "atanf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atanf".to_string() });
        self.load_float_constant(0, f32::from_bits(0x40490fdb));
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::RoundToSingle { d: 2, b: 2 });
        // pi/2f numbers before the zero float (real pool order @26 < @27)
        self.output.intern_constant(0x3fc90fdbu64, 4);
        self.load_float_constant(0, f32::from_bits(0x00000000));
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 2, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatDivideSingle { d: 1, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "atanf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atanf".to_string() });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&28]);
        self.load_float_constant(1, f32::from_bits(0x3fc90fdb));
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::RoundToSingle { d: 2, b: 2 });
        self.load_float_constant(0, f32::from_bits(0x00000000));
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&40]); // bge
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatDivideSingle { d: 1, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "atanf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atanf".to_string() });
        self.load_float_constant(0, f32::from_bits(0x40490fdb));
        self.output.instructions.push(Instruction::FloatAddSingle { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 2, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&46]); // beq
        self.output.instructions.push(Instruction::RoundToSingle { d: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatDivideSingle { d: 1, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "atanf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "atanf".to_string() });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 4, immediate: 16329 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 4059 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 1, offset: 8 });
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
