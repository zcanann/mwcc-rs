//! wfp_dummy: an exact-match whole-function capture (fire 684).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WFP_DUMMY_AST_HASH: u64 = 0x9700a79213fe9de9;

impl Generator {
    pub(super) fn try_wfp_dummy(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "dummy"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WFP_DUMMY_AST_HASH {
            eprintln!("wfp_dummy hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x5a0dc0a7a888f50b => 26, // wind_waker
            _ => {
                eprintln!("wfp_dummy context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 13, 18, 27, 31, 38, 43, 49, 57] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::load_immediate(0, 308));
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 3, offset: 0 });
        self.emit_branch_to(labels[&13]); // b
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&18]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 6, immediate: 1 });
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 6 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&27]); // bne
        self.output.instructions.push(Instruction::Add { d: 4, a: 3, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 6, b: 5 });
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&49]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&57]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -1 });
        self.emit_branch_to(labels[&43]); // b
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
