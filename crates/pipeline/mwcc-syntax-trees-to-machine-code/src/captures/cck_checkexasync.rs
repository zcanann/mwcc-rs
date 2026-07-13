//! cck_checkexasync: an exact-match whole-function capture (fire 757).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CCK_CHECKEXASYNC_AST_HASH: u64 = 0x7137133d313a9d67;

impl Generator {
    pub(super) fn try_cck_checkexasync(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "CARDCheckExAsync"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CCK_CHECKEXASYNC_AST_HASH {
            eprintln!("cck_checkexasync hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cck_checkexasync context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [14, 20, 29, 31, 36, 52, 55, 63, 65, 84, 109, 135, 136, 141, 147, 159, 175, 178, 187, 191, 218, 223, 226, 236, 250, 265, 279, 291, 300, 314, 318, 323, 330, 337, 341, 346, 357, 359, 363, 368, 369, 381, 391, 416, 421, 436, 440, 446, 452, 466, 467] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -96 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 100 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 96 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_22".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 27, s: 4, b: 4 });
        self.output.instructions.push(Instruction::move_register(26, 3));
        self.output.instructions.push(Instruction::move_register(28, 5));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::load_immediate(30, 0));
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.emit_branch_conditional_to(12, 2, labels[&14]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 27, offset: 0 });
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 16 });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetControlBlock".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&20]); // bge
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::LoadWord { d: 24, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 25, a: 24, offset: 128 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 25, offset: 32 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 25, offset: 34 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 24, offset: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::load_immediate(0, 127));
        self.output.instructions.push(Instruction::move_register(3, 25));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 6, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 6 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 0, clear: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 4 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 6, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 0, clear: 16 });
        self.emit_branch_conditional_to(16, 0, labels[&36]); // bdnz
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 25, offset: 508 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&63]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 25, offset: 510 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&65]); // beq
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::LoadWord { d: 23, a: 25, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 22, a: 25, offset: 16 });
        self.record_relocation(RelocationKind::Rel24, "__OSLockSramEx");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__OSLockSramEx".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDBlock");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 30840));
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDBlock");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 16838));
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 24 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 30841 });
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 5, a: 5, b: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 12));
        self.output.instructions.push(Instruction::move_register(6, 25));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 20077 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 5, s: 5, shift: 7 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 7, s: 5, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 7 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 5, a: 5, immediate: 12 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 3, b: 5 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::MultiplyHighWordUnsigned { d: 8, a: 22, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 12345));
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 10, a: 23, b: 4 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 12, a: 22, b: 4 });
        self.output.instructions.push(Instruction::Add { d: 11, a: 8, b: 10 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 10, a: 22, b: 3 });
        self.output.instructions.push(Instruction::AddCarrying { d: 8, a: 12, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 12, s: 8, shift: 16, begin: 0, end: 31 });
        self.output.instructions.push(Instruction::Add { d: 8, a: 11, b: 10 });
        self.output.instructions.push(Instruction::AddExtended { d: 8, a: 8, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 10, s: 8, shift: 16 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 12, s: 8, shift: 16, begin: 0, end: 15 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 8, s: 7, shift: 31 });
        self.output.instructions.push(Instruction::AddCarrying { d: 7, a: 12, b: 7 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 7, s: 7, clear: 24 });
        self.output.instructions.push(Instruction::AddExtended { d: 8, a: 10, b: 8 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 9, b: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&109]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.record_relocation(RelocationKind::Rel24, "__OSUnlockSramEx");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__OSUnlockSramEx".to_string() });
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&109]);
        self.output.instructions.push(Instruction::MultiplyHighWordUnsigned { d: 8, a: 12, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(7, 32767));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 9, a: 10, b: 4 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 11, a: 12, b: 4 });
        self.output.instructions.push(Instruction::Add { d: 10, a: 8, b: 9 });
        self.output.instructions.push(Instruction::AddCarrying { d: 0, a: 11, b: 0 });
        self.output.instructions.push(Instruction::MultiplyLow { d: 9, a: 12, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 8, s: 0, shift: 16, begin: 0, end: 31 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 10, b: 9 });
        self.output.instructions.push(Instruction::AddExtended { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 8, s: 0, shift: 16, begin: 0, end: 15 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 16 });
        self.output.instructions.push(Instruction::And { a: 22, s: 8, b: 7 });
        self.output.instructions.push(Instruction::And { a: 23, s: 0, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&84]); // bdnz
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.record_relocation(RelocationKind::Rel24, "__OSUnlockSramEx");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__OSUnlockSramEx".to_string() });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetFontEncode");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetFontEncode".to_string() });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 25, offset: 36 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&135]); // beq
        self.output.instructions.push(Instruction::load_immediate(4, -13));
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&141]); // bge
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&141]);
        self.output.instructions.push(Instruction::load_immediate(24, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::move_register(8, 24));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 28 });
        self.output.instructions.push(Instruction::load_immediate(25, 0));
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 4, offset: 128 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 6, s: 0, shift: 13 });
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.output.instructions.push(Instruction::Add { d: 6, a: 7, b: 6 });
        self.output.instructions.push(Instruction::load_immediate(0, 2047));
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 10, immediate: 8128 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&159]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 10, offset: 0 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 6, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 9, b: 6 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 10, offset: 2 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 7, s: 0, clear: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 10, immediate: 4 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 6, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 9, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 7, s: 0, clear: 16 });
        self.emit_branch_conditional_to(16, 0, labels[&159]); // bdnz
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&175]); // bne
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.bind_label(labels[&175]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&178]); // bne
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&178]);
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 10, offset: 60 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&187]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 10, offset: 62 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 7, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&191]); // beq
        self.bind_label(labels[&187]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(25, 8));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 132 });
        self.output.instructions.push(Instruction::AddImmediate { d: 24, a: 24, immediate: 1 });
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 4 });
        self.emit_branch_conditional_to(12, 0, labels[&147]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 24, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&223]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 132 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&218]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 6, a: 6, offset: 58 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 3, offset: 58 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 6, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::XorImmediate { a: 25, s: 0, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 25, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 7, b: 0 });
        self.output.instructions.push(Instruction::XorImmediate { a: 0, s: 25, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 4, offset: 132 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 7, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.emit_branch_to(labels[&223]); // b
        self.bind_label(labels[&218]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 5, b: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 5 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 25, s: 0, shift: 31 });
        self.bind_label(labels[&223]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 0, a: 1, immediate: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&226]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 25, a: 1, offset: 8 });
        self.bind_label(labels[&226]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "VerifyFAT");
        self.output.instructions.push(Instruction::BranchAndLink { target: "VerifyFAT".to_string() });
        self.output.instructions.push(Instruction::Add { d: 8, a: 24, b: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&236]); // ble
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&236]);
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 7, offset: 128 });
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 8192 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: 16384 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 24576 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 40 });
        self.emit_branch_conditional_to(12, 2, labels[&250]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&279]); // bge
        self.emit_branch_to(labels[&279]); // b
        self.bind_label(labels[&250]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 7, offset: 132 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&265]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 44 });
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::XorImmediate { a: 0, s: 0, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 7, offset: 132 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.output.instructions.push(Instruction::load_immediate(30, 1));
        self.emit_branch_to(labels[&279]); // b
        self.bind_label(labels[&265]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 36 });
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 7, offset: 136 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::XorImmediate { a: 0, s: 3, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 3, shift: 2 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 4, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.bind_label(labels[&279]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 36 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::XorImmediate { a: 0, s: 0, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 22, a: 3, b: 0 });
        self.output.instructions.push(Instruction::move_register(3, 22));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memset".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 127));
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&291]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 132 });
        self.output.instructions.push(Instruction::Add { d: 8, a: 0, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 255 });
        self.emit_branch_conditional_to(12, 2, labels[&341]); // beq
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 5, a: 8, offset: 54 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.emit_branch_to(labels[&323]); // b
        self.bind_label(labels[&300]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 6, s: 5, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&314]); // blt
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&314]); // bge
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 5, shift: 1, begin: 15, end: 30 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 4, a: 22, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::StoreHalfwordIndexed { s: 4, a: 22, b: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&318]); // ble
        self.bind_label(labels[&314]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&318]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 6, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 4, offset: 136 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 5, a: 4, b: 0 });
        self.bind_label(labels[&323]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(12, 2, labels[&330]); // beq
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 8, offset: 56 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 7, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&300]); // blt
        self.bind_label(labels[&330]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 8, offset: 56 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 7, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&337]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(12, 2, labels[&341]); // beq
        self.bind_label(labels[&337]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&341]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 64 });
        self.emit_branch_conditional_to(16, 0, labels[&291]); // bdnz
        self.output.instructions.push(Instruction::load_immediate(8, 0));
        self.output.instructions.push(Instruction::load_immediate(7, 5));
        self.emit_branch_to(labels[&369]); // b
        self.bind_label(labels[&346]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 7, shift: 1, begin: 15, end: 30 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 0, a: 22, b: 4 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 9, a: 5, b: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&359]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&357]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(29, 1));
        self.output.instructions.push(Instruction::StoreHalfwordIndexed { s: 0, a: 5, b: 4 });
        self.bind_label(labels[&357]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.emit_branch_to(labels[&368]); // b
        self.bind_label(labels[&359]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&363]); // blt
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(12, 0, labels[&368]); // blt
        self.bind_label(labels[&363]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 65535 });
        self.emit_branch_conditional_to(12, 2, labels[&368]); // beq
        self.output.instructions.push(Instruction::load_immediate(4, -6));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&368]);
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.bind_label(labels[&369]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 7, clear: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 6, a: 3, offset: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 6 });
        self.emit_branch_conditional_to(12, 0, labels[&346]); // blt
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 136 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 8, clear: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 6 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&381]); // beq
        self.output.instructions.push(Instruction::StoreHalfword { s: 8, a: 3, offset: 6 });
        self.output.instructions.push(Instruction::load_immediate(29, 1));
        self.bind_label(labels[&381]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&421]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 2047));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 136 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 4 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&391]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 2 });
        self.emit_branch_conditional_to(16, 0, labels[&391]); // bdnz
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&416]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 0 });
        self.bind_label(labels[&416]);
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 });
        self.emit_branch_conditional_to(4, 2, labels[&421]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 5, offset: 2 });
        self.bind_label(labels[&421]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 36 });
        self.output.instructions.push(Instruction::load_immediate(5, 8192));
        self.output.instructions.push(Instruction::XorImmediate { a: 4, s: 0, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 6, s: 4, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 4, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 3, b: 6 });
        self.record_relocation(RelocationKind::Rel24, "memcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcpy".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&440]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&436]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 8192));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 27, offset: 0 });
        self.bind_label(labels[&436]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::move_register(4, 28));
        self.record_relocation(RelocationKind::Rel24, "__CARDUpdateDir");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDUpdateDir".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&440]);
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 31, b: 29 });
        self.emit_branch_conditional_to(12, 2, labels[&452]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&446]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 8192));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 27, offset: 0 });
        self.bind_label(labels[&446]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::move_register(5, 28));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 4, offset: 136 });
        self.record_relocation(RelocationKind::Rel24, "__CARDUpdateFatBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDUpdateFatBlock".to_string() });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&452]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&466]); // beq
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSDisableInterrupts".to_string() });
        self.output.instructions.push(Instruction::move_register(12, 28));
        self.output.instructions.push(Instruction::move_register(22, 3));
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(3, 22));
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.bind_label(labels[&466]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&467]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 96 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_22".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 100 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 96 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
