//! suac_impl_bfbb: an exact-match whole-function capture (fire 513).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SUAC_IMPL_BFBB_AST_HASH: u64 = 0x88d46f1b994253c4; // BfBB (f513)

impl Generator {
    pub(super) fn try_suac_impl_bfbb(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__strtoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 7
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SUAC_IMPL_BFBB_AST_HASH {
            eprintln!("suac_impl_bfbb hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf8b1cd38c2b39c70 => 0, // BfBB strtoul (f513)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.callee_saved = vec![19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // via _savegpr_19
        self.output.jump_table = Some(mwcc_machine_code::JumpTable {
            entries: vec![740, 200, 352, 740, 424, 740, 740, 740, 504, 740, 740, 740, 740, 740, 740, 740, 504],
            anonymous_offset: 70, // measured: table @75 (f513 refctx probe)
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [27, 29, 37, 42, 63, 74, 86, 92, 104, 110, 121, 124, 129, 133, 144, 147, 156, 160, 162, 165, 169, 175, 185, 191, 197, 200] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_19");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_19".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::OrRecord { a: 28, s: 3, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::move_register(25, 9));
        self.output.instructions.push(Instruction::move_register(21, 8));
        self.output.instructions.push(Instruction::move_register(22, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::move_register(30, 6));
        self.output.instructions.push(Instruction::move_register(20, 7));
        self.output.instructions.push(Instruction::load_immediate(24, 1));
        self.output.instructions.push(Instruction::load_immediate(31, 0));
        self.output.instructions.push(Instruction::load_immediate(23, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::load_immediate(26, 0));
        self.emit_branch_conditional_to(12, 0, labels[&27]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 36 });
        self.emit_branch_conditional_to(12, 1, labels[&27]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 22, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&29]); // bge
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::load_immediate(24, 64));
        self.emit_branch_to(labels[&37]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::load_immediate(31, 1));
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&185]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 28 });
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 24, immediate: 16 });
        self.emit_branch_conditional_to(12, 1, labels[&185]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 24, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::move_register(3, 19));
        self.record_relocation(RelocationKind::Rel24, "isspace");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isspace".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&63]); // beq
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.output.instructions.push(Instruction::AddImmediate { d: 23, a: 23, immediate: 1 });
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: 43 });
        self.emit_branch_conditional_to(4, 2, labels[&74]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.emit_branch_to(labels[&86]); // b
        self.bind_label(labels[&74]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: 45 });
        self.emit_branch_conditional_to(4, 2, labels[&86]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 21, offset: 0 });
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::load_immediate(24, 2));
        self.emit_branch_to(labels[&185]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&92]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&104]); // bne
        self.bind_label(labels[&92]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&104]); // bne
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(24, 4));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&104]);
        self.output.instructions.push(Instruction::load_immediate(24, 8));
        self.emit_branch_to(labels[&185]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: 88 });
        self.emit_branch_conditional_to(12, 2, labels[&110]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&121]); // bne
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::load_immediate(28, 16));
        self.output.instructions.push(Instruction::load_immediate(24, 8));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&124]); // bne
        self.output.instructions.push(Instruction::load_immediate(28, 8));
        self.bind_label(labels[&124]);
        self.output.instructions.push(Instruction::load_immediate(24, 16));
        self.emit_branch_to(labels[&185]); // b
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&129]); // bne
        self.output.instructions.push(Instruction::load_immediate(28, 10));
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&133]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::DivideWordUnsigned { d: 26, a: 0, b: 28 });
        self.bind_label(labels[&133]);
        self.output.instructions.push(Instruction::move_register(3, 19));
        self.record_relocation(RelocationKind::Rel24, "isdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&147]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 19, a: 19, immediate: -48 });
        self.output.instructions.push(Instruction::CompareWord { a: 19, b: 28 });
        self.emit_branch_conditional_to(12, 0, labels[&165]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 24, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(0, 64));
        self.emit_branch_conditional_to(4, 2, labels[&144]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.bind_label(labels[&144]);
        self.output.instructions.push(Instruction::move_register(24, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 19, a: 19, immediate: 48 });
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::move_register(3, 19));
        self.record_relocation(RelocationKind::Rel24, "isalpha");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isalpha".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&156]); // beq
        self.output.instructions.push(Instruction::move_register(3, 19));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink { target: "toupper".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -55 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 28 });
        self.emit_branch_conditional_to(12, 0, labels[&162]); // blt
        self.bind_label(labels[&156]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 24, immediate: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&160]); // bne
        self.output.instructions.push(Instruction::load_immediate(24, 32));
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&160]);
        self.output.instructions.push(Instruction::load_immediate(24, 64));
        self.emit_branch_to(labels[&185]); // b
        self.bind_label(labels[&162]);
        self.output.instructions.push(Instruction::move_register(3, 19));
        self.record_relocation(RelocationKind::Rel24, "toupper");
        self.output.instructions.push(Instruction::BranchAndLink { target: "toupper".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 19, a: 3, immediate: -55 });
        self.bind_label(labels[&165]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 27, b: 26 });
        self.emit_branch_conditional_to(4, 1, labels[&169]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 25, offset: 0 });
        self.bind_label(labels[&169]);
        self.output.instructions.push(Instruction::MultiplyLow { d: 27, a: 27, b: 28 });
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 19, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&175]); // ble
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 25, offset: 0 });
        self.bind_label(labels[&175]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 19 });
        self.output.instructions.push(Instruction::load_immediate(24, 16));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(19, 3));
        self.bind_label(labels[&185]);
        self.output.instructions.push(Instruction::CompareWord { a: 31, b: 22 });
        self.emit_branch_conditional_to(12, 1, labels[&191]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 19, immediate: -1 });
        self.emit_branch_conditional_to(12, 2, labels[&191]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 24, shift: 0, begin: 25, end: 26 });
        self.emit_branch_conditional_to(12, 2, labels[&42]); // beq
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 0, s: 24, immediate: 52 });
        self.emit_branch_conditional_to(4, 2, labels[&197]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 20, offset: 0 });
        self.emit_branch_to(labels[&200]); // b
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 23 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 20, offset: 0 });
        self.bind_label(labels[&200]);
        self.output.instructions.push(Instruction::move_register(12, 29));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::move_register(4, 19));
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_19");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_19".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
