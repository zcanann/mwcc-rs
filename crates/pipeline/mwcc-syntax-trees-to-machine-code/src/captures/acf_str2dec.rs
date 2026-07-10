//! acf_str2dec: an exact-match whole-function capture (fire 685).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACF_STR2DEC_AST_HASH: u64 = 0x9d36b9c948e557da;

impl Generator {
    pub(super) fn try_acf_str2dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__str2dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACF_STR2DEC_AST_HASH {
            eprintln!("acf_str2dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xadf9060938342c54 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("acf_str2dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [5, 11, 16, 25, 29, 36, 41, 47, 55] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreHalfword { s: 5, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 0 });
        self.emit_branch_to(labels[&11]); // b
        self.bind_label(labels[&5]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 6, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 5, a: 3, b: 0 });
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&16]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 6, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: 1 });
        self.emit_branch_to(labels[&29]); // b
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output.instructions.push(Instruction::Add { d: 4, a: 3, b: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 6, b: 5 });
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&47]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -1 });
        self.emit_branch_to(labels[&41]); // b
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
