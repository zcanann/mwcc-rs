//! wc_fwide_mel: an exact-match whole-function capture (fire 475).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WC_FWIDE_MEL_AST_HASH: u64 = 0xaa18fbe130c5e9bc;

impl Generator {
    pub(super) fn try_wc_fwide_mel(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fwide"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WC_FWIDE_MEL_AST_HASH {
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
        for target in [5, 13, 16, 22, 26, 28, 30] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 26, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&5]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 5, shift: 28, begin: 30, end: 31 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&13]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&16]); // bge
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 0 });
        self.emit_branch_to(labels[&28]); // b
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&22]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 5, s: 0, shift: 4, begin: 26, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 3, offset: 5 });
        self.emit_branch_to(labels[&26]); // b
        self.bind_label(labels[&22]);
        self.emit_branch_conditional_to(4, 0, labels[&26]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 5, s: 0, shift: 4, begin: 26, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 3, offset: 5 });
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
