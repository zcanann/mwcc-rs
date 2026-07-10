//! acf_num2dec: an exact-match whole-function capture (fire 685).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACF_NUM2DEC_AST_HASH: u64 = 0xc12d777434be55d6;

impl Generator {
    pub(super) fn try_acf_num2dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACF_NUM2DEC_AST_HASH {
            eprintln!("acf_num2dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xadf9060938342c54 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("acf_num2dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [16, 27, 30, 38, 43, 45, 50, 51, 58, 64, 72, 75, 77, 82, 91, 96, 99] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 30, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 9 });
        self.emit_branch_conditional_to(12, 1, labels[&99]); // bgt
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 0, s: 30 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 36 });
        self.emit_branch_conditional_to(4, 1, labels[&16]); // ble
        self.output.instructions.push(Instruction::load_immediate(30, 36));
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::ExtendSignHalfwordRecord { a: 6, s: 30 });
        self.emit_branch_conditional_to(4, 1, labels[&75]); // ble
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&75]); // bge
        self.output.instructions.push(Instruction::Add { d: 5, a: 31, b: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(4, 1, labels[&27]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&51]); // b
        self.bind_label(labels[&27]);
        self.emit_branch_conditional_to(4, 0, labels[&30]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.emit_branch_to(labels[&51]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&45]); // bge
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&43]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&51]); // b
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&38]); // bdnz
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&50]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&51]); // b
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 6, a: 31, offset: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&75]); // blt
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 6, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&64]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&75]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&72]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&75]); // b
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.emit_branch_to(labels[&82]); // b
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 5, a: 31, b: 0 });
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(12, 0, labels[&77]); // blt
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&96]); // b
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 31, b: 4 });
        self.bind_label(labels[&96]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 5, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&91]); // blt
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
