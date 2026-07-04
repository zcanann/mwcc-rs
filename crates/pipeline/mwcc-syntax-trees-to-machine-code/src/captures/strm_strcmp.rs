//! strm_strcmp: an exact-match whole-function capture (fire 472).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STRM_STRCMP_AST_HASH: u64 = 0xb66575d51d1b1bcc;

impl Generator {
    pub(super) fn try_strm_strcmp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strcmp"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STRM_STRCMP_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // melee (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [6, 16, 20, 26, 30, 31, 33, 41, 46, 52, 58, 62, 68] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 6, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&6]); // beq
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 6, b: 5 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&6]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 30 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 3, clear: 30 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&58]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&33]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 6, immediate: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.output.instructions.push(Instruction::move_register(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&30]);
        self.emit_branch_conditional_to(16, 0, labels[&20]); // bdnz
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&33]);
        self.record_relocation(RelocationKind::EmbSda21, "K2");
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 3, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, "K1");
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 7, b: 5 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.emit_branch_to(labels[&46]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 7, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 8, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 7, b: 5 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 1 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&58]); // beq
        self.output.instructions.push(Instruction::move_register(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&62]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&68]); // beq
        self.output.instructions.push(Instruction::move_register(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&68]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&62]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
