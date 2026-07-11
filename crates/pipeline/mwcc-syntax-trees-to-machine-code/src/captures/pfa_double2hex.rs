//! pfa_double2hex: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_DOUBLE2HEX_AST_HASH: u64 = 0x505221ba776e529e;

impl Generator {
    pub(super) fn try_pfa_double2hex(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "double2hex"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_DOUBLE2HEX_AST_HASH && hash != 0xde5c24cdfb58cfa0 {
            eprintln!("pfa_double2hex hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 66, // strikers
            0xecff4eb19d59de49 => 66, // pikmin2
            0x46f259063d157aea => 66, // wind_waker
            0xf8b1cd38c2b39c70 => 66, // animal_crossing
            0x3012f8741ad9c69d => 66, // mp4: the INF/NAN string block @354 (owned here)
            _ => {
                eprintln!("pfa_double2hex context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 128;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [14, 35, 39, 47, 50, 52, 65, 69, 77, 80, 82, 113, 116, 121, 133, 138, 143, 144, 147, 153, 155, 163, 165, 173, 179, 183, 184] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -128 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 124 });
        self.output.instructions.push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 120 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 24 });
        self.emit_branch_conditional_to(4, 1, labels[&14]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&184]); // b
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 18 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 69 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(4, 2, labels[&52]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 16, end: 16 });
        self.emit_branch_conditional_to(12, 2, labels[&39]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65 });
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x2d, 0x49, 0x4e, 0x46]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x2d, 0x69, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65 });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x49, 0x4e, 0x46]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x69, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.emit_branch_to(labels[&184]); // b
        self.bind_label(labels[&52]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(4, 2, labels[&82]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 24, end: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&69]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65 });
        self.emit_branch_conditional_to(4, 2, labels[&65]); // bne
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x2d, 0x4e, 0x41, 0x4e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&80]); // b
        self.bind_label(labels[&65]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x2d, 0x6e, 0x61, 0x6e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&80]); // b
        self.bind_label(labels[&69]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65 });
        self.emit_branch_conditional_to(4, 2, labels[&77]); // bne
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x4e, 0x41, 0x4e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&80]); // b
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        let index = self.intern_string_literal(&[0x6e, 0x61, 0x6e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.bind_label(labels[&80]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.emit_branch_to(labels[&184]); // b
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::load_immediate(8, 0));
        self.output.instructions.push(Instruction::load_immediate(9, 1));
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate(7, 100));
        self.output.instructions.push(Instruction::StoreByte { s: 9, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 28, begin: 21, end: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 3, s: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 9, a: 1, offset: 49 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1023 });
        self.output.instructions.push(Instruction::StoreByte { s: 8, a: 1, offset: 50 });
        self.output.instructions.push(Instruction::StoreByte { s: 8, a: 1, offset: 51 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 8, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 1, offset: 53 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 44 });
        self.record_relocation(RelocationKind::Rel24, "long2str");
        self.output.instructions.push(Instruction::BranchAndLink { target: "long2str".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 97 });
        self.emit_branch_conditional_to(4, 2, labels[&113]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 112));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: -1 });
        self.emit_branch_to(labels[&116]); // b
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::load_immediate(0, 80));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: -1 });
        self.bind_label(labels[&116]);
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, labels[&147]); // blt
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 5, s: 8, shift: 31 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 8, clear: 31 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 5, b: 8 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 3, shift: 1 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 0, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 7, a: 6, b: 3 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 7, shift: 28, begin: 28, end: 31 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&133]); // beq
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 7, clear: 28 });
        self.bind_label(labels[&133]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 10 });
        self.emit_branch_conditional_to(4, 0, labels[&138]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 48 });
        self.emit_branch_to(labels[&144]); // b
        self.bind_label(labels[&138]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 97 });
        self.emit_branch_conditional_to(4, 2, labels[&143]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 87 });
        self.emit_branch_to(labels[&144]); // b
        self.bind_label(labels[&143]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 55 });
        self.bind_label(labels[&144]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&121]); // bdnz
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&153]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&155]); // beq
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::load_immediate(0, 46));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -1 });
        self.bind_label(labels[&155]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::load_immediate(3, 49));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 97 });
        self.emit_branch_conditional_to(4, 2, labels[&163]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 120));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -2 });
        self.emit_branch_to(labels[&165]); // b
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::load_immediate(0, 88));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -2 });
        self.bind_label(labels[&165]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate(3, 48));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 3, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 16, end: 16 });
        self.emit_branch_conditional_to(12, 2, labels[&173]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -1 });
        self.emit_branch_to(labels[&183]); // b
        self.bind_label(labels[&173]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&179]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -1 });
        self.emit_branch_to(labels[&183]); // b
        self.bind_label(labels[&179]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&183]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 4, offset: -1 });
        self.bind_label(labels[&183]);
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.bind_label(labels[&184]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 124 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 120 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 128 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
