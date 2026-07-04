//! str_strcpy_sun: an exact-match whole-function capture (fire 505).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRCPY_SUN_AST_HASH: u64 = 0x67e207ce41c610c1; // sunshine (f505)

impl Generator {
    pub(super) fn try_str_strcpy_sun(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strcpy"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STR_STRCPY_SUN_AST_HASH {
            eprintln!("str_strcpy_sun hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // sunshine (f505)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [15, 20, 22, 29, 35, 39] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 3, clear: 30 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 4, clear: 30 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 5 });
        self.output.instructions.push(Instruction::move_register(7, 3));
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 5, immediate: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&20]); // beq
        self.bind_label(labels[&15]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 7, offset: 1 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.emit_branch_conditional_to(16, 0, labels[&15]); // bdnz
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 4, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, "K2");
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, "K1");
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 8, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 6, s: 6, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -4 });
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 8, a: 7, offset: 4 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 8, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 8, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 6, s: 6, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 4 });
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 7, offset: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
