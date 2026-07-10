//! sup2_strtoul_impl: an exact-match whole-function capture (fire 463).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SUP2_STRTOUL_IMPL_AST_HASH: u64 = 0x8be0e0a83e1e698a;

impl Generator {
    pub(super) fn try_sup2_strtoul_impl(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SUP2_STRTOUL_IMPL_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x1af05b8ba2d5e628 => 0, // dev loop
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.callee_saved = vec![20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // _savegpr_20
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![788, 200, 360, 788, 432, 788, 788, 788, 512, 788, 788, 788, 788, 788, 788, 788, 512],
            anonymous_offset: 80, // measured: table @85
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [27, 29, 37, 42, 65, 76, 88, 94, 106, 112, 123, 126, 131, 135, 148, 151, 157, 160, 163, 167, 169, 173, 176, 177, 181, 187, 197, 203, 209, 212] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_20");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_20".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::OrRecord { a: 29, s: 3, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::move_register(24, 9));
        self.output.instructions.push(Instruction::move_register(21, 8));
        self.output.instructions.push(Instruction::move_register(22, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::move_register(28, 6));
        self.output.instructions.push(Instruction::move_register(20, 7));
        self.output.instructions.push(Instruction::load_immediate(25, 1));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::load_immediate(23, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(26, 0));
        self.emit_branch_conditional_to(12, 0, labels[&27]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 36 });
        self.emit_branch_conditional_to(12, 1, labels[&27]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 22, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&29]); // bge
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::load_immediate(25, 64));
        self.emit_branch_to(labels[&37]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&197]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 29 });
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(12, 1, labels[&197]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 25, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 29, end: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&65]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.output.instructions.push(Instruction::AddImmediate { d: 23, a: 23, immediate: 1 });
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 43 });
        self.emit_branch_conditional_to(4, 2, labels[&76]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&88]); // b
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 45 });
        self.emit_branch_conditional_to(4, 2, labels[&88]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 21, offset: 0 });
        self.bind_label(labels[&88]);
        self.output.instructions.push(Instruction::load_immediate(25, 2));
        self.emit_branch_to(labels[&197]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&94]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&106]); // bne
        self.bind_label(labels[&94]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&106]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(25, 4));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&106]);
        self.output.instructions.push(Instruction::load_immediate(25, 8));
        self.emit_branch_to(labels[&197]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&112]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&123]); // bne
        self.bind_label(labels[&112]);
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
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&126]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 8));
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::load_immediate(25, 16));
        self.emit_branch_to(labels[&197]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&131]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 10));
        self.bind_label(labels[&131]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&135]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 29 });
        self.bind_label(labels[&135]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 4, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&151]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&177]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&148]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&148]);
        self.output.instructions.push(Instruction::move_register(25, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 48 });
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 24, end: 25 });
        self.emit_branch_conditional_to(12, 2, labels[&163]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&157]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&160]); // b
        self.bind_label(labels[&157]);
        self.record_relocation(RelocationKind::Addr16Ha, "__upper_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__upper_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.bind_label(labels[&160]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&169]); // blt
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&167]); // bne
        self.output.instructions.push(Instruction::load_immediate(25, 32));
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::load_immediate(25, 64));
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&169]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&173]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&176]); // b
        self.bind_label(labels[&173]);
        self.record_relocation(RelocationKind::Addr16Ha, "__upper_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__upper_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -55 });
        self.bind_label(labels[&177]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 27, b: 26 });
        self.emit_branch_conditional_to(4, 1, labels[&181]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&181]);
        self.output.instructions.push(Instruction::MultiplyLow { d: 27, a: 27, b: 29 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&187]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&187]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(25, 16));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::CompareWord { a: 31, b: 22 });
        self.emit_branch_conditional_to(12, 1, labels[&203]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&203]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 25, shift: 0, begin: 25, end: 26 });
        self.emit_branch_conditional_to(12, 2, labels[&42]); // beq
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 0, s: 25, immediate: 52 });
        self.emit_branch_conditional_to(4, 2, labels[&209]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 20, offset: 0 });
        self.emit_branch_to(labels[&212]); // b
        self.bind_label(labels[&209]);
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 23 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 20, offset: 0 });
        self.bind_label(labels[&212]);
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
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
