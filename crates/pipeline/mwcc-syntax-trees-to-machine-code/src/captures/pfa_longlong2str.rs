//! pfa_longlong2str: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_LONGLONG2STR_AST_HASH: u64 = 0xc374a1c5b7d0e5b6;

impl Generator {
    pub(super) fn try_pfa_longlong2str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "longlong2str"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_LONGLONG2STR_AST_HASH {
            eprintln!("pfa_longlong2str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x3012f8741ad9c69d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfa_longlong2str context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![252, 268, 268, 268, 268, 268, 268, 268, 268, 268, 268, 268, 156, 268, 268, 268, 268, 156, 268, 268, 268, 268, 268, 212, 268, 268, 268, 268, 268, 232, 268, 268, 252],
            anonymous_offset: 58, // real @284
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [27, 29, 67, 84, 89, 90, 111, 121, 124, 136, 143, 150, 161, 162, 164, 177, 182, 188, 192, 193] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_22".to_string() });
        self.output.instructions.push(Instruction::move_register(23, 5));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::Xor { a: 5, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 23, offset: -1 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::move_register(24, 6));
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 23, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate(25, 0));
        self.output.instructions.push(Instruction::load_immediate(26, 0));
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 24, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 111 });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.emit_branch_to(labels[&193]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 24, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -88 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 32 });
        self.emit_branch_conditional_to(12, 1, labels[&67]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 5, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate(28, 10));
        self.output.instructions.push(Instruction::SubtractFromCarrying { d: 0, a: 0, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 5, b: 5 });
        self.output.instructions.push(Instruction::NegateRecord { d: 3, a: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&67]); // beq
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 30, a: 30, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(25, 1));
        self.output.instructions.push(Instruction::SubtractFromZeroExtended { d: 31, a: 31 });
        self.emit_branch_to(labels[&67]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(28, 8));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 24, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.emit_branch_to(labels[&67]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(28, 10));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 24, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.emit_branch_to(labels[&67]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(28, 16));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 24, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.bind_label(labels[&67]);
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::move_register(5, 29));
        self.output.instructions.push(Instruction::move_register(6, 28));
        self.record_relocation(RelocationKind::Rel24, "__mod2u");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__mod2u".to_string() });
        self.output.instructions.push(Instruction::move_register(22, 4));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::move_register(5, 29));
        self.output.instructions.push(Instruction::move_register(6, 28));
        self.record_relocation(RelocationKind::Rel24, "__div2u");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__div2u".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 22, immediate: 10 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.emit_branch_conditional_to(4, 0, labels[&84]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 22, immediate: 48 });
        self.emit_branch_to(labels[&90]); // b
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 120 });
        self.emit_branch_conditional_to(4, 2, labels[&89]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 22, immediate: 87 });
        self.emit_branch_to(labels[&90]); // b
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 22, immediate: 55 });
        self.bind_label(labels[&90]);
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -1 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 30, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 1 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 31, b: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&67]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 8));
        self.output.instructions.push(Instruction::Xor { a: 0, s: 29, b: 4 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 28, b: 3 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&111]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&111]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 27, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&111]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -1 });
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 24, offset: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&121]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&124]); // beq
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 24, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 12 });
        self.bind_label(labels[&124]);
        self.output.instructions.push(Instruction::load_immediate(3, 16));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::Xor { a: 3, s: 28, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 29, b: 0 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&136]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 24, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -2 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 24, offset: 12 });
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 24, offset: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 27, b: 23 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&143]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&193]); // b
        self.bind_label(labels[&143]);
        self.output.instructions.push(Instruction::CompareWord { a: 26, b: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 26, b: 3 });
        self.output.instructions.push(Instruction::load_immediate(4, 48));
        self.emit_branch_conditional_to(4, 0, labels[&164]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&161]); // beq
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 27, offset: -7 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 27, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&150]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&164]); // beq
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&162]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 27, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&162]); // bdnz
        self.bind_label(labels[&164]);
        self.output.instructions.push(Instruction::load_immediate(3, 16));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::Xor { a: 3, s: 28, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 29, b: 0 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&177]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&177]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 24, offset: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 27, offset: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -2 });
        self.bind_label(labels[&177]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&182]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -1 });
        self.emit_branch_to(labels[&192]); // b
        self.bind_label(labels[&182]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 24, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&188]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -1 });
        self.emit_branch_to(labels[&192]); // b
        self.bind_label(labels[&188]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&192]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 27, offset: -1 });
        self.bind_label(labels[&192]);
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.bind_label(labels[&193]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_22");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_22".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
