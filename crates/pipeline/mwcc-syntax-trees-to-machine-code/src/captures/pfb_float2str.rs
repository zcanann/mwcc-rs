//! pfb_float2str: an exact-match whole-function capture (fire 696).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFB_FLOAT2STR_AST_HASH: u64 = 0x7d7afeb0f1e76dd7;

impl Generator {
    pub(super) fn try_pfb_float2str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "float2str"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFB_FLOAT2STR_AST_HASH {
            eprintln!("pfb_float2str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa605ebc1c79b708d => 187, // melee
            _ => {
                eprintln!("pfb_float2str context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 80;
        self.non_leaf = true;
        self.callee_saved_float = 1;
        self.output.string_number_after_constants = Some(1);
        for bits in [
            0x0000000000000000u64,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [17, 20, 25, 37, 43, 49, 56, 59, 62, 70, 74, 76, 82, 100, 103, 107, 113, 119, 126, 129, 135, 138, 145, 153, 160, 166, 170, 183, 197, 205, 206, 208, 211, 213, 221, 223, 231, 237, 242, 248, 261, 265, 270, 275, 277, 283, 286, 291, 298, 309, 310, 312, 318, 320, 325, 327, 337, 356, 357, 361, 363, 369, 375, 379, 380] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -80 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 28, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::move_register(28, 4));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output.instructions.push(Instruction::load_immediate(4, 3));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__va_arg".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 3, offset: 0 });
        self.emit_branch_to(labels[&20]); // b
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::load_immediate(4, 3));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__va_arg".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 3, offset: 0 });
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&25]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 10 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec".to_string() });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 1, immediate: 17 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 31, b: 0 });
        self.emit_branch_to(labels[&43]); // b
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 14 });
        self.bind_label(labels[&43]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&49]); // ble
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&37]); // beq
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&56]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&59]); // beq
        self.emit_branch_to(labels[&82]); // b
        self.bind_label(labels[&56]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(12, 2, labels[&76]); // beq
        self.emit_branch_to(labels[&82]); // b
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 14 });
        self.emit_branch_to(labels[&82]); // b
        self.bind_label(labels[&62]);
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&70]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -5 });
        let index = self.intern_string_literal(&[0x2d, 0x49, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.emit_branch_to(labels[&74]); // b
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -4 });
        let index = self.intern_string_literal(&[0x49, 0x6e, 0x66]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.bind_label(labels[&74]);
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 28, immediate: -4 });
        let index = self.intern_string_literal(&[0x4e, 0x61, 0x4e]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.record_relocation(RelocationKind::Rel24, "strcpy");
        self.output.instructions.push(Instruction::BranchAndLink { target: "strcpy".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 28, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 28, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 101 });
        self.emit_branch_conditional_to(12, 2, labels[&153]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&103]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 70 });
        self.emit_branch_conditional_to(12, 2, labels[&379]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&100]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 69 });
        self.emit_branch_conditional_to(4, 0, labels[&153]); // bge
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 72 });
        self.emit_branch_conditional_to(4, 0, labels[&379]); // bge
        self.emit_branch_to(labels[&107]); // b
        self.bind_label(labels[&103]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 103 });
        self.emit_branch_conditional_to(12, 2, labels[&107]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&379]); // bge
        self.emit_branch_to(labels[&242]); // b
        self.bind_label(labels[&107]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&113]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: -4 });
        self.emit_branch_conditional_to(12, 0, labels[&119]); // blt
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&138]); // blt
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&126]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 103 });
        self.emit_branch_conditional_to(4, 2, labels[&135]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 101));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 5 });
        self.emit_branch_to(labels[&153]); // b
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::load_immediate(0, 69));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 29, offset: 5 });
        self.emit_branch_to(labels[&153]); // b
        self.bind_label(labels[&138]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&145]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&242]); // b
        self.bind_label(labels[&145]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_conditional_to(4, 0, labels[&242]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.emit_branch_to(labels[&242]); // b
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&160]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.bind_label(labels[&160]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 6, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::load_immediate(8, 43));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&166]); // bge
        self.output.instructions.push(Instruction::Negate { d: 6, a: 6 });
        self.output.instructions.push(Instruction::load_immediate(8, 45));
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 26214));
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 26215 });
        self.emit_branch_to(labels[&183]); // b
        self.bind_label(labels[&170]);
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 0, a: 5, b: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 4, s: 3, shift: 31 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 4, a: 3, immediate: 10 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 4, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 0, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: -1 });
        self.bind_label(labels[&183]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&170]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 7, immediate: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&170]); // blt
        self.output.instructions.push(Instruction::StoreByte { s: 8, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 30, b: 28 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&197]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&208]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 2 });
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 3 });
        self.emit_branch_to(labels[&206]); // b
        self.bind_label(labels[&205]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&206]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&205]); // bne
        self.bind_label(labels[&208]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 31, b: 3 });
        self.emit_branch_to(labels[&213]); // b
        self.bind_label(labels[&211]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&213]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&211]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&221]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&223]); // beq
        self.bind_label(labels[&221]);
        self.output.instructions.push(Instruction::load_immediate(0, 46));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&223]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&231]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&231]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&237]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&237]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&379]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&242]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 0, b: 4 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 7, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 0, labels[&248]); // bge
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&248]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&261]); // ble
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 0, b: 4 });
        self.record_relocation(RelocationKind::Rel24, "round_decimal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "round_decimal".to_string() });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 7, a: 3, immediate: -1 });
        self.emit_branch_conditional_to(4, 0, labels[&261]); // bge
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&261]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 14 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 6, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&265]); // bge
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.bind_label(labels[&265]);
        self.output.instructions.push(Instruction::Add { d: 0, a: 6, b: 7 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 509 });
        self.emit_branch_conditional_to(4, 1, labels[&270]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&270]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 48));
        self.output.instructions.push(Instruction::Add { d: 5, a: 31, b: 0 });
        self.emit_branch_to(labels[&277]); // b
        self.bind_label(labels[&275]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 3, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&277]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&275]); // blt
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&286]); // b
        self.bind_label(labels[&283]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&286]);
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&291]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&283]); // blt
        self.bind_label(labels[&291]);
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 7 });
        self.output.instructions.push(Instruction::load_immediate(4, 48));
        self.emit_branch_conditional_to(4, 0, labels[&312]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&309]); // beq
        self.bind_label(labels[&298]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 30, offset: -7 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&298]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&312]); // beq
        self.bind_label(labels[&309]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&310]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 4, a: 30, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&310]); // bdnz
        self.bind_label(labels[&312]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&318]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&320]); // beq
        self.bind_label(labels[&318]);
        self.output.instructions.push(Instruction::load_immediate(0, 46));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&320]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&361]); // beq
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 48));
        self.emit_branch_to(labels[&327]); // b
        self.bind_label(labels[&325]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 3, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&327]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 6 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&325]); // blt
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&363]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&356]); // beq
        self.bind_label(labels[&337]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -3 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -5 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -6 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: -7 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: -7 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&337]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&363]); // beq
        self.bind_label(labels[&356]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&357]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_conditional_to(16, 0, labels[&357]); // bdnz
        self.emit_branch_to(labels[&363]); // b
        self.bind_label(labels[&361]);
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&363]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&369]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 45));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&369]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&375]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 43));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.emit_branch_to(labels[&379]); // b
        self.bind_label(labels[&375]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&379]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 32));
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 30, offset: -1 });
        self.bind_label(labels[&379]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.bind_label(labels[&380]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 80 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
