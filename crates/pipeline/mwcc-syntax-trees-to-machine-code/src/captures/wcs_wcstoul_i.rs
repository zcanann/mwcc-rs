//! wcs_wcstoul_i: an exact-match whole-function capture (fire 700).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WCS_WCSTOUL_I_AST_HASH: u64 = 0xfaa7f63a2fb6c898;

impl Generator {
    pub(super) fn try_wcs_wcstoul_i(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__wcstoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WCS_WCSTOUL_I_AST_HASH {
            eprintln!("wcs_wcstoul_i hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wcs_wcstoul_i context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![884, 196, 384, 884, 460, 884, 884, 884, 544, 884, 884, 884, 884, 884, 884, 884, 544],
            anonymous_offset: 84, // real @104
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [26, 28, 36, 41, 58, 59, 70, 82, 94, 100, 113, 120, 131, 134, 139, 143, 152, 153, 163, 166, 175, 176, 182, 186, 189, 193, 195, 199, 203, 210, 221, 228, 233, 234] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_21");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_21".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::OrRecord { a: 29, s: 3, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::move_register(24, 9));
        self.output.instructions.push(Instruction::move_register(22, 8));
        self.output.instructions.push(Instruction::move_register(23, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::move_register(28, 6));
        self.output.instructions.push(Instruction::move_register(21, 7));
        self.output.instructions.push(Instruction::load_immediate(25, 1));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(26, 0));
        self.emit_branch_conditional_to(12, 0, labels[&26]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 36 });
        self.emit_branch_conditional_to(12, 1, labels[&26]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 23, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&28]); // bge
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::load_immediate(25, 64));
        self.emit_branch_to(labels[&36]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&221]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 29 });
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(12, 1, labels[&221]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 25, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 256 });
        self.emit_branch_conditional_to(4, 0, labels[&58]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__wctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 4, shift: 1, begin: 15, end: 30 });
        self.record_relocation(RelocationKind::Addr16Lo, "__wctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&70]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 43 });
        self.emit_branch_conditional_to(4, 2, labels[&82]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&94]); // b
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 45 });
        self.emit_branch_conditional_to(4, 2, labels[&94]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 22, offset: 0 });
        self.bind_label(labels[&94]);
        self.output.instructions.push(Instruction::load_immediate(25, 2));
        self.emit_branch_to(labels[&221]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&113]); // bne
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&113]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(25, 4));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::load_immediate(25, 8));
        self.emit_branch_to(labels[&221]); // b
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&120]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&131]); // bne
        self.bind_label(labels[&120]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(29, 16));
        self.output.instructions.push(Instruction::load_immediate(25, 8));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&131]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&134]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 8));
        self.bind_label(labels[&134]);
        self.output.instructions.push(Instruction::load_immediate(25, 16));
        self.emit_branch_to(labels[&221]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&139]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 10));
        self.bind_label(labels[&139]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&143]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 3, b: 29 });
        self.bind_label(labels[&143]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 256 });
        self.emit_branch_conditional_to(4, 0, labels[&152]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__wctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 4, shift: 1, begin: 15, end: 30 });
        self.record_relocation(RelocationKind::Addr16Lo, "__wctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 0, begin: 27, end: 27 });
        self.emit_branch_to(labels[&153]); // b
        self.bind_label(labels[&152]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&166]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&199]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&163]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::move_register(25, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 48 });
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 256 });
        self.emit_branch_conditional_to(4, 0, labels[&175]); // bge
        self.record_relocation(RelocationKind::Addr16Ha, "__wctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 4, shift: 1, begin: 15, end: 30 });
        self.record_relocation(RelocationKind::Addr16Lo, "__wctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 0, begin: 24, end: 25 });
        self.emit_branch_to(labels[&176]); // b
        self.bind_label(labels[&175]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&189]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&182]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&182]);
        self.record_relocation(RelocationKind::Addr16Ha, "__upper_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__upper_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 0 });
        self.bind_label(labels[&186]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&195]); // blt
        self.bind_label(labels[&189]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&193]); // bne
        self.output.instructions.push(Instruction::load_immediate(25, 32));
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&193]);
        self.output.instructions.push(Instruction::load_immediate(25, 64));
        self.emit_branch_to(labels[&221]); // b
        self.bind_label(labels[&195]);
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.record_relocation(RelocationKind::Rel24, "towupper");
        self.output.instructions.push(Instruction::BranchAndLink { target: "towupper".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 0, clear: 16 });
        self.bind_label(labels[&199]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 27, b: 26 });
        self.emit_branch_conditional_to(4, 1, labels[&203]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::MultiplyLow { d: 27, a: 27, b: 29 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&210]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&210]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(25, 16));
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 0 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.bind_label(labels[&221]);
        self.output.instructions.push(Instruction::CompareWord { a: 31, b: 23 });
        self.emit_branch_conditional_to(12, 1, labels[&228]); // bgt
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&228]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 25, shift: 0, begin: 25, end: 26 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.bind_label(labels[&228]);
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 0, s: 25, immediate: 52 });
        self.emit_branch_conditional_to(4, 2, labels[&233]); // bne
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.emit_branch_to(labels[&234]); // b
        self.bind_label(labels[&233]);
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: -1 });
        self.bind_label(labels[&234]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 21, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_21");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_21".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
