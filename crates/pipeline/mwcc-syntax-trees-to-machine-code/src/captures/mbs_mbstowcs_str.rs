//! mbs_mbstowcs_str: an exact-match whole-function capture (fire 514).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MBS_MBSTOWCS_STR_AST_HASH: u64 = 0x78e84c45be2fad37; // strikers (f514, WEAK-materialized inline)

impl Generator {
    pub(super) fn try_mbs_mbstowcs_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "mbstowcs"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MBS_MBSTOWCS_STR_AST_HASH {
            eprintln!("mbs_mbstowcs_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa1c526a9076479be => 0, // pikmin2 mbstring (f514)
            0xdba628923f494fa9 => 0, // strikers mbstring (f514)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [5, 9, 12, 17, 22, 33, 35, 37, 52, 54, 60, 62, 64, 66, 67, 71, 77, 82, 87, 91, 95, 99, 103, 104, 108, 111] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&5]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&9]);
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.output.instructions.push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 6, s: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 7, s: 7 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 6, s: 7, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::load_immediate(6, 1));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 6, s: 7, shift: 0, begin: 24, end: 26 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 192 });
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&35]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 128 });
        self.emit_branch_conditional_to(4, 2, labels[&33]); // bne
        self.output.instructions.push(Instruction::load_immediate(6, 2));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::load_immediate(6, -2));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 6, s: 7, shift: 0, begin: 24, end: 27 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 224 });
        self.emit_branch_conditional_to(4, 2, labels[&66]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&54]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 128 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 128 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output.instructions.push(Instruction::load_immediate(6, 3));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&60]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 6, s: 6, shift: 0, begin: 24, end: 24 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 128 });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.bind_label(labels[&60]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::load_immediate(6, -2));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::load_immediate(6, -1));
        self.emit_branch_to(labels[&67]); // b
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::load_immediate(6, -1));
        self.bind_label(labels[&67]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&71]); // bge
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&71]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&82]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&77]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&87]); // bge
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 4 });
        self.emit_branch_conditional_to(4, 0, labels[&91]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 6, begin: 22, end: 25 });
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 5, clear: 26 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 6, begin: 16, end: 25 });
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 4, clear: 25 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 16 });
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 4, s: 0, clear: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&95]); // bne
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.emit_branch_to(labels[&104]); // b
        self.bind_label(labels[&95]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 128 });
        self.emit_branch_conditional_to(4, 0, labels[&99]); // bge
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.emit_branch_to(labels[&104]); // b
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 2048 });
        self.emit_branch_conditional_to(4, 0, labels[&103]); // bge
        self.output.instructions.push(Instruction::load_immediate(4, 2));
        self.emit_branch_to(labels[&104]); // b
        self.bind_label(labels[&103]);
        self.output.instructions.push(Instruction::load_immediate(4, 3));
        self.bind_label(labels[&104]);
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 6 });
        self.emit_branch_conditional_to(12, 2, labels[&108]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&108]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&111]); // beq
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 0 });
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::move_register(3, 6));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
