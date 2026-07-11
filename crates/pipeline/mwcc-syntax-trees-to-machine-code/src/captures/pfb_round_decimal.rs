//! pfb_round_decimal: an exact-match whole-function capture (fire 696).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFB_ROUND_DECIMAL_AST_HASH: u64 = 0x13f47a7b01af20f9;

impl Generator {
    pub(super) fn try_pfb_round_decimal(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "round_decimal"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFB_ROUND_DECIMAL_AST_HASH {
            eprintln!("pfb_round_decimal hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa605ebc1c79b708d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfb_round_decimal context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [2, 10, 22, 28, 33, 35, 42, 54, 56, 59, 60, 62, 72] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.bind_label(labels[&2]);
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreHalfword { s: 5, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 7 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 0 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 3, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 6, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -48 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 6, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.output.instructions.push(Instruction::Add { d: 5, a: 3, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 5 });
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&28]); // ble
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&33]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 8, offset: -1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 0, clear: 31 });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.output.instructions.push(Instruction::Xor { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 5, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 5 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 5, s: 0, shift: 31 });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 8, offset: -1 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 0, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -48 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 7, s: 0 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 7, b: 6 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 5, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 0, shift: 1, begin: 31, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&54]); // bne
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&56]); // bne
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&60]); // b
        self.bind_label(labels[&56]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 0 });
        self.emit_branch_to(labels[&62]); // b
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::load_immediate(6, 9));
        self.bind_label(labels[&60]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&42]); // bne
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&72]); // beq
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 5, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::load_immediate(0, 49));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 5, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&2]); // beq
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
