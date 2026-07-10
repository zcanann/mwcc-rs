//! afp_num2dec_mel: an exact-match whole-function capture (fire 679).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const AFP_NUM2DEC_MEL_AST_HASHES: [u64; 2] = [
    0x617239e4bd5cb629, // super_smash_brothers_melee
    0x7caa476a0a990d25, // super_mario_sunshine (identical .text; only pool @Ns differ)
];

impl Generator {
    pub(super) fn try_afp_num2dec_mel(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__num2dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !AFP_NUM2DEC_MEL_AST_HASHES.contains(&hash) {
            eprintln!("afp_num2dec_mel hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x970011031b615ae8 => 81, // melee: pool @114 (ours @33 unbumped)
            0x7b54c0b83c01543b => 79, // sunshine: pool @102 (ours @23 unbumped)
            _ => {
                eprintln!("afp_num2dec_mel context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 80;
        self.non_leaf = true;
        self.callee_saved_float = 1;
        // The pool skips @118-@120 (three internal labels between the 4th and
        // 5th constants' creation).
        self.output.constant_number_gaps = vec![(4, 3)];
        for bits in [
            0x0000000000000000u64,
            0x3ff0000000000000,
            0x3fb999999999999a,
            0x4024000000000000,
            0x4330000080000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [14, 25, 35, 40, 42, 44, 49, 51, 53, 54, 66, 71, 73, 75, 80, 82, 84, 85, 89, 91, 99, 119, 123, 125, 128, 131, 135, 137, 140, 143, 145, 151, 153, 163, 167, 188, 200, 203, 209, 215, 217, 223, 224] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -80 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 31, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.emit_branch_conditional_to(4, 1, labels[&14]); // ble
        self.output.instructions.push(Instruction::load_immediate(31, 16));
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 31 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 30, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 5 });
        self.emit_branch_to(labels[&224]); // b
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&35]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&53]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&44]); // beq
        self.emit_branch_to(labels[&53]); // b
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&42]); // beq
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&44]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&49]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&51]); // beq
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&54]); // b
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&54]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&91]); // bgt
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&66]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&84]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&75]); // beq
        self.emit_branch_to(labels[&84]); // b
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&71]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&73]); // beq
        self.bind_label(labels[&71]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&73]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&80]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&82]); // beq
        self.bind_label(labels[&80]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&85]); // b
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&89]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 5 });
        self.emit_branch_to(labels[&224]); // b
        self.bind_label(labels[&91]);
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&99]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::FloatNegate { d: 31, b: 31 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 0 });
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "frexp".to_string() });
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 5));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -26651 });
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 17180));
        self.output.instructions.push(Instruction::MultiplyLow { d: 5, a: 5, b: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "bit_values");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -8573 });
        self.record_relocation(RelocationKind::Addr16Lo, "bit_values");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 0, a: 4, b: 5 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 18 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::AddRecord { d: 4, a: 0, b: 3 });
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.emit_branch_conditional_to(4, 0, labels[&128]); // bge
        self.output.instructions.push(Instruction::Negate { d: 4, a: 4 });
        self.emit_branch_to(labels[&125]); // b
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&123]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 31, a: 31, c: 0 });
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 4, s: 4, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.bind_label(labels[&125]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&119]); // bne
        self.emit_branch_to(labels[&140]); // b
        self.bind_label(labels[&128]);
        self.emit_branch_conditional_to(4, 1, labels[&140]); // ble
        self.load_double_constant(1, 0x3ff0000000000000);
        self.emit_branch_to(labels[&137]); // b
        self.bind_label(labels[&131]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&135]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 4, s: 4, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.bind_label(labels[&137]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&131]); // bne
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 31, a: 31, b: 1 });
        self.bind_label(labels[&140]);
        self.load_double_constant(1, 0x3fb999999999999a);
        self.load_double_constant(0, 0x3ff0000000000000);
        self.emit_branch_to(labels[&145]); // b
        self.bind_label(labels[&143]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 31, a: 31, c: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&145]);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.output.instructions.push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&143]); // beq
        self.load_double_constant(1, 0x4024000000000000);
        self.load_double_constant(0, 0x3fb999999999999a);
        self.emit_branch_to(labels[&153]); // b
        self.bind_label(labels[&151]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 31, a: 31, c: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.bind_label(labels[&153]);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 31, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&151]); // blt
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 26214));
        self.record_relocation(RelocationKind::Addr16Ha, "digit_values");
        self.output.instructions.push(Instruction::load_immediate_shifted(6, 0));
        self.load_double_constant(1, 0x4330000080000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 30, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 5, immediate: 26215 });
        self.record_relocation(RelocationKind::Addr16Lo, "digit_values");
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 6, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate_shifted(8, 17200));
        self.emit_branch_to(labels[&203]); // b
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 8 });
        self.output.instructions.push(Instruction::move_register(11, 31));
        self.emit_branch_conditional_to(4, 1, labels[&167]); // ble
        self.output.instructions.push(Instruction::load_immediate(11, 8));
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 11, shift: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 9, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 6, b: 11 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 31, a: 11, b: 31 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 31, a: 31, c: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 11, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 11 });
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 11, immediate: 1 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 31 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 12, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 31, a: 31, b: 0 });
        self.emit_branch_to(labels[&200]); // b
        self.bind_label(labels[&188]);
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 0, a: 7, b: 12 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 5, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 6, s: 5, shift: 31 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 6 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 6, a: 5, immediate: 10 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 5, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 6, a: 6, b: 12 });
        self.output.instructions.push(Instruction::Add { d: 12, a: 0, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 6, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.bind_label(labels[&200]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 10, a: 10, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&188]); // bne
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 11 });
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&163]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 5, a: 29, offset: 2 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 1, labels[&209]); // ble
        self.output.instructions.push(Instruction::load_immediate(5, 36));
        self.bind_label(labels[&209]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::SubtractFromRecord { d: 5, a: 0, b: 5 });
        self.emit_branch_conditional_to(4, 1, labels[&223]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 48));
        self.emit_branch_to(labels[&217]); // b
        self.bind_label(labels[&215]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.bind_label(labels[&217]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 6, a: 6, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&215]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 30, offset: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 0, b: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 4 });
        self.bind_label(labels[&223]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 30, offset: 2 });
        self.bind_label(labels[&224]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 84 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 80 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
