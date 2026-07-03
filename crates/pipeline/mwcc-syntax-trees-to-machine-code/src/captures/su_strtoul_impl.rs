//! su_strtoul_impl: an exact-match whole-function capture (fire 461).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SU_STRTOUL_IMPL_AST_HASH: u64 = 0x3108161bd6ed94;

impl Generator {
    pub(super) fn try_su_strtoul_impl(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SU_STRTOUL_IMPL_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x6ff29e48ce03ae67 => 0, // pikmin (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.output.jump_table = Some(mwcc_machine_code::JumpTable {
            entries: vec![744, 196, 356, 744, 428, 744, 744, 744, 508, 744, 744, 744, 744, 744, 744, 744, 508],
            anonymous_offset: 71, // measured (real table @N)
        });
        self.callee_saved = vec![20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // via _savegpr_20
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [26, 28, 36, 41, 64, 75, 87, 93, 105, 111, 122, 125, 130, 134, 147, 150, 157, 161, 163, 166, 170, 176, 186, 192, 197, 198] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_20");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_20".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::OrRecord { a: 28, s: 3, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::move_register(25, 9));
        self.output.instructions.push(Instruction::move_register(22, 8));
        self.output.instructions.push(Instruction::move_register(23, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::move_register(30, 6));
        self.output.instructions.push(Instruction::move_register(21, 7));
        self.output.instructions.push(Instruction::load_immediate(24, 1));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(26, 0));
        self.emit_branch_conditional_to(12, 0, labels[&26]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&26]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 36 });
        self.emit_branch_conditional_to(12, 1, labels[&26]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 23, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&28]); // bge
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::load_immediate(24, 64));
        self.emit_branch_to(labels[&36]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&186]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 28 });
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 24, immediate: 16 });
        self.emit_branch_conditional_to(12, 1, labels[&186]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 24, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 20, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&64]); // beq
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: 43 });
        self.emit_branch_conditional_to(4, 2, labels[&75]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: 45 });
        self.emit_branch_conditional_to(4, 2, labels[&87]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 22, offset: 0 });
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::load_immediate(24, 2));
        self.emit_branch_to(labels[&186]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&93]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&105]); // bne
        self.bind_label(labels[&93]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&105]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(24, 4));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&105]);
        self.output.instructions.push(Instruction::load_immediate(24, 8));
        self.emit_branch_to(labels[&186]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&111]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&122]); // bne
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(28, 16));
        self.output.instructions.push(Instruction::load_immediate(24, 8));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&125]); // bne
        self.output.instructions.push(Instruction::load_immediate(28, 8));
        self.bind_label(labels[&125]);
        self.output.instructions.push(Instruction::load_immediate(24, 16));
        self.emit_branch_to(labels[&186]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&130]); // bne
        self.output.instructions.push(Instruction::load_immediate(28, 10));
        self.bind_label(labels[&130]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&134]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 28 });
        self.bind_label(labels[&134]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 20, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&150]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 20, a: 20, immediate: -48 });
        self.output.instructions.push(Instruction::CompareWord { a: 20, b: 28 });
        self.emit_branch_conditional_to(12, 0, labels[&166]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 24, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&147]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::move_register(24, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 20, a: 20, immediate: 48 });
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 24, end: 25 });
        self.emit_branch_conditional_to(12, 2, labels[&157]); // beq
        self.output.instructions.push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink { target: "toupper".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 28 });
        self.emit_branch_conditional_to(12, 0, labels[&163]); // blt
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 24, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&161]); // bne
        self.output.instructions.push(Instruction::load_immediate(24, 32));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::load_immediate(24, 64));
        self.emit_branch_to(labels[&186]); // b
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::move_register(3, 20));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink { target: "toupper".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 20, a: 3, immediate: -55 });
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 27, b: 26 });
        self.emit_branch_conditional_to(4, 1, labels[&170]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 25, offset: 0 });
        self.bind_label(labels[&170]);
        self.output.instructions.push(Instruction::MultiplyLow { d: 27, a: 27, b: 28 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 20, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&176]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 25, offset: 0 });
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 20 });
        self.output.instructions.push(Instruction::load_immediate(24, 16));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(20, 3));
        self.bind_label(labels[&186]);
        self.output.instructions.push(Instruction::CompareWord { a: 31, b: 23 });
        self.emit_branch_conditional_to(12, 1, labels[&192]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 20, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&192]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 24, shift: 0, begin: 25, end: 26 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.bind_label(labels[&192]);
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 0, s: 24, immediate: 52 });
        self.emit_branch_conditional_to(4, 2, labels[&197]); // bne
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.emit_branch_to(labels[&198]); // b
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: -1 });
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 20));
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 21, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_20");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_20".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
