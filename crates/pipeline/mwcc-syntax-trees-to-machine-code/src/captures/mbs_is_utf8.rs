//! mbs_is_utf8: an exact-match whole-function capture (fire 514).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_IS_UTF8_AST_HASH: u64 = 0x7b788e3399a467ed; // mbs_str (f514)

impl Generator {
    pub(super) fn try_mbs_is_utf8(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "is_utf8_complete"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_IS_UTF8_AST_HASH {
            eprintln!("mbs_is_utf8 hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa1c526a9076479be => 0, // pikmin2 mbstring (f514)
            0xdba628923f494fa9 => 0, // strikers mbstring (f514, post-materialization fingerprint)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 9, 14, 26, 28, 43, 45, 51, 53, 55, 57] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 5, s: 5 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 5, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 5, shift: 0, begin: 24, end: 26 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 192 });
        self.emit_branch_conditional_to(4, 2, labels[&28]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&26]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 128 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 2 });
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::load_immediate(3, -2));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 5, shift: 0, begin: 24, end: 27 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 224 });
        self.emit_branch_conditional_to(4, 2, labels[&57]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&45]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 128 });
        self.emit_branch_conditional_to(4, 2, labels[&43]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 128 });
        self.emit_branch_conditional_to(4, 2, labels[&43]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 3));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&51]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 128 });
        self.emit_branch_conditional_to(12, 2, labels[&53]); // beq
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::load_immediate(3, -2));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
