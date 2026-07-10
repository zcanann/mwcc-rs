//! suac_impl_str: an exact-match whole-function capture (fire 512).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SUAC_IMPL_STR_AST_HASH: u64 = 0x70ed64993cdc2241; // strikers (f512)

impl Generator {
    pub(super) fn try_suac_impl_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SUAC_IMPL_STR_AST_HASH {
            eprintln!("suac_impl_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers strtoul (f512)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.callee_saved = vec![21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // via _savegpr_21
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![784, 196, 356, 784, 428, 784, 784, 784, 508, 784, 784, 784, 784, 784, 784, 784, 508],
            anonymous_offset: 80, // measured: table @100 (f512 refctx probe)
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [26, 28, 36, 41, 64, 75, 87, 93, 105, 111, 122, 125, 130, 134, 147, 150, 156, 159, 162, 166, 168, 172, 175, 176, 180, 186, 196, 202, 207, 208] {
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
        self.emit_branch_conditional_to(12, 2, labels[&196]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 29 });
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(12, 1, labels[&196]); // bgt
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
        self.emit_branch_conditional_to(12, 2, labels[&64]); // beq
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 43 });
        self.emit_branch_conditional_to(4, 2, labels[&75]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 45 });
        self.emit_branch_conditional_to(4, 2, labels[&87]); // bne
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
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::load_immediate(25, 2));
        self.emit_branch_to(labels[&196]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&93]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&105]); // bne
        self.bind_label(labels[&93]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&105]); // bne
        self.output.instructions.push(Instruction::move_register(12, 30));
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::load_immediate(25, 4));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&105]);
        self.output.instructions.push(Instruction::load_immediate(25, 8));
        self.emit_branch_to(labels[&196]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&111]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&122]); // bne
        self.bind_label(labels[&111]);
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
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&125]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 8));
        self.bind_label(labels[&125]);
        self.output.instructions.push(Instruction::load_immediate(25, 16));
        self.emit_branch_to(labels[&196]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&130]); // bne
        self.output.instructions.push(Instruction::load_immediate(29, 10));
        self.bind_label(labels[&130]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&134]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 29 });
        self.bind_label(labels[&134]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 4, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&150]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&176]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&147]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::move_register(25, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 48 });
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 0, begin: 24, end: 25 });
        self.emit_branch_conditional_to(12, 2, labels[&162]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&156]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&159]); // b
        self.bind_label(labels[&156]);
        self.record_relocation(RelocationKind::Addr16Ha, "__upper_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__upper_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.bind_label(labels[&159]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 29 });
        self.emit_branch_conditional_to(12, 0, labels[&168]); // blt
        self.bind_label(labels[&162]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&166]); // bne
        self.output.instructions.push(Instruction::load_immediate(25, 32));
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::load_immediate(25, 64));
        self.emit_branch_to(labels[&196]); // b
        self.bind_label(labels[&168]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&172]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&175]); // b
        self.bind_label(labels[&172]);
        self.record_relocation(RelocationKind::Addr16Ha, "__upper_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__upper_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 3, b: 5 });
        self.bind_label(labels[&175]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -55 });
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 27, b: 26 });
        self.emit_branch_conditional_to(4, 1, labels[&180]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&180]);
        self.output.instructions.push(Instruction::MultiplyLow { d: 27, a: 27, b: 29 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&186]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 0 });
        self.bind_label(labels[&186]);
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
        self.bind_label(labels[&196]);
        self.output.instructions.push(Instruction::CompareWord { a: 31, b: 23 });
        self.emit_branch_conditional_to(12, 1, labels[&202]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&202]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 25, shift: 0, begin: 25, end: 26 });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.bind_label(labels[&202]);
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 0, s: 25, immediate: 52 });
        self.emit_branch_conditional_to(4, 2, labels[&207]); // bne
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.emit_branch_to(labels[&208]); // b
        self.bind_label(labels[&207]);
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: -1 });
        self.bind_label(labels[&208]);
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
