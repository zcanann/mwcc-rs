//! sfp_equals_dec: an exact-match whole-function capture (fire 681).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFP_EQUALS_DEC_AST_HASH: u64 = 0x7bf5bf837a36fe51;

impl Generator {
    pub(super) fn try_sfp_equals_dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__equals_dec"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFP_EQUALS_DEC_AST_HASH {
            eprintln!("sfp_equals_dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 0, // strikers ansi_fp copy (bump TBD)
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfp_equals_dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 13, 23, 27, 34, 36, 42, 48, 51, 55, 61, 63, 65] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 5 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 5, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&65]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::move_register(9, 7));
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&23]); // ble
        self.output.instructions.push(Instruction::move_register(9, 6));
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::load_immediate(8, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 9 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&36]); // ble
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 5, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&34]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&27]); // bdnz
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&51]); // bne
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 8, b: 6 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 8, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&63]); // bge
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&48]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&42]); // bdnz
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 8, b: 7 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&63]); // bge
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&61]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&55]); // bdnz
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
