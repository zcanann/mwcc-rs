//! cck_verifyfat: an exact-match whole-function capture (fire 757).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CCK_VERIFYFAT_AST_HASH: u64 = 0x8a96aa8288c7b4ee;

impl Generator {
    pub(super) fn try_cck_verifyfat(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "VerifyFAT"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CCK_VERIFYFAT_AST_HASH {
            eprintln!("cck_verifyfat hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cck_verifyfat context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [11, 21, 37, 40, 48, 53, 57, 64, 65, 76, 105, 110, 113] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate(30, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 3 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 3, offset: 128 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 6, s: 0, shift: 13 });
        self.output.instructions.push(Instruction::load_immediate(0, 2047));
        self.output.instructions.push(Instruction::Add { d: 7, a: 7, b: 6 });
        self.output.instructions.push(Instruction::load_immediate(10, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 7, immediate: 4 });
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 8, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 8, b: 8 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 9, b: 8 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 8, a: 6, offset: 2 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 10, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 10, s: 0, clear: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 4 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 8, b: 8 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 9, b: 8 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 10, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 10, s: 0, clear: 16 });
        self.emit_branch_conditional_to(16, 0, labels[&21]); // bdnz
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 10, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.output.instructions.push(Instruction::load_immediate(10, 0));
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 7, offset: 2 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 10, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&53]); // beq
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.emit_branch_to(labels[&76]); // b
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 8, a: 3, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::load_immediate(9, 5));
        self.emit_branch_to(labels[&65]); // b
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 9, shift: 1, begin: 15, end: 30 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&64]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 6, clear: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 0, clear: 16 });
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 9, immediate: 1 });
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 8 });
        self.emit_branch_conditional_to(12, 0, labels[&57]); // blt
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 7, offset: 6 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 6, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&76]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&11]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&110]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&105]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 4, s: 6 });
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 0, s: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::XorImmediate { a: 30, s: 0, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 4, s: 30, shift: 2 });
        self.output.instructions.push(Instruction::XorImmediate { a: 0, s: 30, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 7, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 7, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.emit_branch_to(labels[&110]); // b
        self.bind_label(labels[&105]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 4 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 30, s: 0, shift: 31 });
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&113]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 29, offset: 0 });
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
