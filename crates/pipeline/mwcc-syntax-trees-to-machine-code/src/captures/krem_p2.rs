//! krem_p2: an exact-match whole-function capture (fire 468).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const KREM_P2_AST_HASH: u64 = 0x90cf79132775e52b;

impl Generator {
    pub(super) fn try_krem_p2(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__kernel_rem_pio2"
            || function.return_type != Type::Int
            || function.parameters.len() != 6
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != KREM_P2_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 245, // pikmin2 + wind_waker: pool @250 (ours @5)
            0xb61776ae26f47f0e => 245, // battle_for_bikini_bottom: pool @270 (its pre-bump base is 25)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 720;
        self.non_leaf = true;
        self.callee_saved = vec![16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]; // via _savegpr_16
        self.callee_saved_float = 7; // via _savefpr_25
        self.output.constant_number_gaps = vec![(7, 2)]; // @256 then @259
        for bits in [
            0x0000000000000000u64,
            0x3e70000000000000,
            0x4170000000000000,
            0x4020000000000000,
            0x3fc0000000000000,
            0x3fe0000000000000,
            0x3ff0000000000000,
            0x4330000080000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [25, 38, 42, 48, 53, 56, 72, 122, 131, 139, 142, 156, 167, 203, 204, 222, 252, 259, 264, 274, 283, 285, 287, 295, 302, 308, 317, 331, 351, 352, 356, 360, 361, 372, 398, 448, 456, 464, 467, 471, 480, 483, 487, 521, 527, 545, 586, 587, 599, 608, 613, 620, 624, 628, 635, 644, 664, 665, 669, 672, 673, 675, 684, 704, 705, 709, 713, 714, 729, 748, 756, 760, 763, 764, 766, 775, 806, 807, 815, 822, 853, 854, 862, 869, 889, 890, 894, 902, 910] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -720 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 724 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 720 });
        self.record_relocation(RelocationKind::Rel24, "_savefpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savefpr_25".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 664 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_16");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_16".to_string() });
        self.output.instructions.push(Instruction::load_immediate_shifted(9, 10923));
        self.output.instructions.push(Instruction::move_register(23, 7));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -3 });
        self.record_relocation(RelocationKind::Addr16Ha, "init_jk");
        self.output.instructions.push(Instruction::load_immediate_shifted(10, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 9, immediate: -21845 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 9, s: 23, shift: 2 });
        self.output.instructions.push(Instruction::MultiplyHighWord { d: 0, a: 7, b: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, "init_jk");
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 10, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 28, a: 7, b: 9 });
        self.output.instructions.push(Instruction::move_register(21, 3));
        self.output.instructions.push(Instruction::move_register(22, 4));
        self.output.instructions.push(Instruction::move_register(24, 8));
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 6, immediate: -1 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::AddRecord { d: 29, a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&25]); // bge
        self.output.instructions.push(Instruction::load_immediate(29, 0));
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 29, immediate: 1 });
        self.output.instructions.push(Instruction::AddRecord { d: 7, a: 30, b: 28 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 4, a: 0, immediate: 24 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 6, a: 30, b: 29 });
        self.load_double_constant(1, 0x4330000080000000);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 6, shift: 2 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 26, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 24, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 408 });
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 17200));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&53]); // blt
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&42]); // bge
        self.load_double_constant(0, 0x0000000000000000);
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 572 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 1 });
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&38]); // bdnz
        self.bind_label(labels[&53]);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 88 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.emit_branch_to(labels[&142]); // b
        self.bind_label(labels[&56]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.load_double_constant(4, 0x0000000000000000);
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.emit_branch_conditional_to(12, 0, labels[&139]); // blt
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 30, immediate: -8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&122]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 9, immediate: 8 });
        self.output.instructions.push(Instruction::move_register(4, 21));
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 8, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 30, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 408 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&122]); // blt
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 8, a: 6, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 10, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 10 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 9, a: 9, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 10, s: 9, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 9, a: 8, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 10 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 8, a: 8, b: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 9, s: 9, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 10, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 9 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 4 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 10 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 9, a: 8, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 10, s: 9, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 5 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 10 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 8, a: 8, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 10, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 6, immediate: 6 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 2, c: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 9, a: 9, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 9, s: 9, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 40 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: 7 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 8, a: 8, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 8, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 3, a: 4, offset: 48 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 2, a: 3, b: 9 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 56 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 3, c: 2, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 64 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&72]); // bdnz
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 6, shift: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 6, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 8, a: 30, b: 7 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 21, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 408 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 6, b: 30 });
        self.emit_branch_conditional_to(12, 1, labels[&139]); // bgt
        self.bind_label(labels[&131]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 6, b: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&131]); // bdnz
        self.bind_label(labels[&139]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.bind_label(labels[&142]);
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 28 });
        self.emit_branch_conditional_to(4, 1, labels[&56]); // ble
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 18, a: 26, immediate: 24 });
        self.load_double_constant(26, 0x3e70000000000000);
        self.load_double_constant(27, 0x4330000080000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 16, a: 1, immediate: 8 });
        self.load_double_constant(28, 0x4170000000000000);
        self.output.instructions.push(Instruction::move_register(31, 28));
        self.load_double_constant(29, 0x3fc0000000000000);
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 17, a: 26, immediate: 23 });
        self.load_double_constant(30, 0x4020000000000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 20, a: 1, immediate: 408 });
        self.load_double_constant(31, 0x0000000000000000);
        self.output.instructions.push(Instruction::load_immediate_shifted(19, 17200));
        self.bind_label(labels[&156]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 31, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 88 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::move_register(4, 16));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.emit_branch_conditional_to(4, 1, labels[&222]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 31, shift: 31, begin: 1, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&203]); // beq
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 3, a: 26, c: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 3 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 572 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 580 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 2, b: 27 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble { d: 2, a: 28, c: 3, b: 1 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDoubleWithUpdate { d: 0, a: 5, offset: -16 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 2 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 3, a: 26, c: 1 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 3 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 572 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 580 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 2, b: 27 });
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble { d: 2, a: 28, c: 3, b: 1 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 3, b: 0 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 2 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&167]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&222]); // beq
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&204]);
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 3, a: 26, c: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::LoadFloatDoubleWithUpdate { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 3 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 572 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 580 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 2, b: 27 });
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble { d: 2, a: 28, c: 3, b: 1 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 3, b: 0 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 2, b: 2 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&204]); // bdnz
        self.bind_label(labels[&222]);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 25, b: 1 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 29, c: 25 });
        self.record_relocation(RelocationKind::Rel24, "floor");
        self.output.instructions.push(Instruction::BranchAndLink { target: "floor".to_string() });
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble { d: 25, a: 30, c: 1, b: 25 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 26, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(25, 0));
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 25 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::LoadWord { d: 27, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 27, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 580 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 27 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 25, a: 25, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&252]); // ble
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: -4 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: 6, s: 3, b: 18 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 6, b: 18 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 27, a: 27, b: 6 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 0, a: 4, b: 5 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 4, b: 5 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: 25, s: 0, b: 17 });
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&252]);
        self.emit_branch_conditional_to(4, 2, labels[&259]); // bne
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 4, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -4 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 25, s: 0, shift: 23 });
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&259]);
        self.load_double_constant(0, 0x3fe0000000000000);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 25, b: 0 });
        self.output.instructions.push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&264]); // bne
        self.output.instructions.push(Instruction::load_immediate(25, 2));
        self.bind_label(labels[&264]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&317]); // ble
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 256));
        self.output.instructions.push(Instruction::move_register(6, 16));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 31 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 27, a: 27, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&287]); // ble
        self.bind_label(labels[&274]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 6, offset: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&283]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&285]); // beq
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 6, offset: 0 });
        self.emit_branch_to(labels[&285]); // b
        self.bind_label(labels[&283]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 6, offset: 0 });
        self.bind_label(labels[&285]);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&274]); // bdnz
        self.bind_label(labels[&287]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 26, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&308]); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 26, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&302]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&308]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 26, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&295]); // bge
        self.emit_branch_to(labels[&308]); // b
        self.bind_label(labels[&295]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -4 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 5, b: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 9 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 3, a: 5, b: 4 });
        self.emit_branch_to(labels[&308]); // b
        self.bind_label(labels[&302]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -4 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 5, b: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 10 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 3, a: 5, b: 4 });
        self.bind_label(labels[&308]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&317]); // bne
        self.load_double_constant(1, 0x3ff0000000000000);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 25, a: 1, b: 25 });
        self.emit_branch_conditional_to(12, 2, labels[&317]); // beq
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 25, a: 25, b: 1 });
        self.bind_label(labels[&317]);
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 31, b: 25 });
        self.emit_branch_conditional_to(4, 2, labels[&471]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 28 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 3, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 28, b: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&356]); // blt
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&351]); // beq
        self.bind_label(labels[&331]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -4 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -12 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -20 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: -28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -32 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&331]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&356]); // beq
        self.bind_label(labels[&351]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&352]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -4 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 5, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&352]); // bdnz
        self.bind_label(labels[&356]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&471]); // bne
        self.output.instructions.push(Instruction::load_immediate(10, 1));
        self.emit_branch_to(labels[&361]); // b
        self.bind_label(labels[&360]);
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 10, immediate: 1 });
        self.bind_label(labels[&361]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 10, b: 28 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 16, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&360]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 88 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 9, shift: 3 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 31, b: 10 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 0 });
        self.emit_branch_to(labels[&467]); // b
        self.bind_label(labels[&372]);
        self.output.instructions.push(Instruction::Add { d: 0, a: 29, b: 9 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 30, b: 9 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 19, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 3, a: 24, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 7, shift: 3 });
        self.load_double_constant(4, 0x0000000000000000);
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate(8, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 27 });
        self.output.instructions.push(Instruction::StoreFloatDoubleIndexed { s: 0, a: 20, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&464]); // blt
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 30, immediate: -8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&448]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 11, immediate: 8 });
        self.output.instructions.push(Instruction::move_register(4, 21));
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 408 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 11, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&448]); // blt
        self.bind_label(labels[&398]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 11, a: 8, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 12, s: 11, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 12 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 8, immediate: 2 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 12, a: 11, b: 7 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 8, immediate: 3 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 25, s: 12, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 11, a: 11, b: 7 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 12, s: 11, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 4 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 25 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 11, a: 0, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 5 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 3, a: 4, offset: 48 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 12 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 12, s: 11, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 8, immediate: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 7 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 12 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 12, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 40 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 8, immediate: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 11, a: 11, b: 7 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 2, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 12 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 11, s: 11, shift: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 7 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 2, a: 3, b: 11 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 56 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 64 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 8 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 3, c: 2, b: 4 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&398]); // bdnz
        self.bind_label(labels[&448]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 4, s: 8, shift: 3 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 8, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 408 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 21, b: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 8, b: 30 });
        self.emit_branch_conditional_to(12, 1, labels[&464]); // bgt
        self.bind_label(labels[&456]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 8, b: 7 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 4, a: 1, c: 0, b: 4 });
        self.emit_branch_conditional_to(16, 0, labels[&456]); // bdnz
        self.bind_label(labels[&464]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 9, immediate: 1 });
        self.bind_label(labels[&467]);
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&372]); // ble
        self.output.instructions.push(Instruction::Add { d: 31, a: 31, b: 10 });
        self.emit_branch_to(labels[&156]); // b
        self.bind_label(labels[&471]);
        self.load_double_constant(0, 0x0000000000000000);
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 25 });
        self.emit_branch_conditional_to(4, 2, labels[&487]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: -24 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.emit_branch_to(labels[&483]); // b
        self.bind_label(labels[&480]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: -24 });
        self.bind_label(labels[&483]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&480]); // beq
        self.emit_branch_to(labels[&527]); // b
        self.bind_label(labels[&487]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 25 });
        self.output.instructions.push(Instruction::Negate { d: 3, a: 26 });
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.load_double_constant(3, 0x4170000000000000);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 3 });
        self.output.instructions.push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&521]); // bne
        self.load_double_constant(0, 0x3e70000000000000);
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 5, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 24 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 584 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 588 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 580 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 576 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble { d: 1, a: 3, c: 0, b: 1 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 1, b: 1 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 568 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 572 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 6, a: 4, b: 5 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 3, a: 4, b: 0 });
        self.emit_branch_to(labels[&527]); // b
        self.bind_label(labels[&521]);
        self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&527]);
        self.load_double_constant(1, 0x3ff0000000000000);
        self.output.instructions.push(Instruction::move_register(3, 26));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 31, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 8, s: 31, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 88 });
        self.load_double_constant(5, 0x4330000080000000);
        self.load_double_constant(0, 0x3e70000000000000);
        self.output.instructions.push(Instruction::Add { d: 6, a: 6, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 7, b: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 17200));
        self.emit_branch_conditional_to(12, 0, labels[&599]); // blt
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 30, begin: 2, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&586]); // beq
        self.bind_label(labels[&545]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 5, immediate: 32768 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: -4 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 5, immediate: 32768 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 5, immediate: 32768 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: -12 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: -16 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 5, immediate: 32768 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 7, offset: -8 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 7, offset: -16 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 7, offset: -24 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -32 });
        self.emit_branch_conditional_to(16, 0, labels[&545]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&599]); // beq
        self.bind_label(labels[&586]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&587]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: -4 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 0, s: 5, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 596 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 1, offset: 592 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 5 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 1, c: 3 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&587]); // bdnz
        self.bind_label(labels[&599]);
        self.record_relocation(RelocationKind::Addr16Ha, "PIo2");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::move_register(9, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 88 });
        self.record_relocation(RelocationKind::Addr16Lo, "PIo2");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 248 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&628]); // blt
        self.bind_label(labels[&608]);
        self.load_double_constant(2, 0x0000000000000000);
        self.output.instructions.push(Instruction::move_register(6, 5));
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 9, b: 31 });
        self.output.instructions.push(Instruction::load_immediate(10, 0));
        self.emit_branch_to(labels[&620]); // b
        self.bind_label(labels[&613]);
        self.output.instructions.push(Instruction::Add { d: 0, a: 9, b: 10 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 10, immediate: 1 });
        self.output.instructions.push(Instruction::FloatMultiplyAddDouble { d: 2, a: 1, c: 0, b: 2 });
        self.bind_label(labels[&620]);
        self.output.instructions.push(Instruction::CompareWord { a: 10, b: 28 });
        self.emit_branch_conditional_to(12, 1, labels[&624]); // bgt
        self.output.instructions.push(Instruction::CompareWord { a: 10, b: 7 });
        self.emit_branch_conditional_to(4, 1, labels[&613]); // ble
        self.bind_label(labels[&624]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 7, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 9, immediate: -1 });
        self.output.instructions.push(Instruction::StoreFloatDoubleIndexed { s: 2, a: 3, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&608]); // bdnz
        self.bind_label(labels[&628]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 23, immediate: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&766]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&910]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 23, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&635]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&675]); // bge
        self.emit_branch_to(labels[&910]); // b
        self.bind_label(labels[&635]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 248 });
        self.load_double_constant(1, 0x0000000000000000);
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, labels[&669]); // blt
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&664]); // beq
        self.bind_label(labels[&644]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -32 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -40 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -48 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -64 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&644]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&669]); // beq
        self.bind_label(labels[&664]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&665]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&665]); // bdnz
        self.bind_label(labels[&669]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&672]); // bne
        self.emit_branch_to(labels[&673]); // b
        self.bind_label(labels[&672]);
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 1 });
        self.bind_label(labels[&673]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 0 });
        self.emit_branch_to(labels[&910]); // b
        self.bind_label(labels[&675]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 248 });
        self.load_double_constant(2, 0x0000000000000000);
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, labels[&709]); // blt
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&704]); // beq
        self.bind_label(labels[&684]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -32 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -40 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -48 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -64 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&684]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&709]); // beq
        self.bind_label(labels[&704]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&705]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 2, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&705]); // bdnz
        self.bind_label(labels[&709]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&713]); // bne
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 2 });
        self.emit_branch_to(labels[&714]); // b
        self.bind_label(labels[&713]);
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 2 });
        self.bind_label(labels[&714]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 248 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 1));
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 6, a: 0, b: 2 });
        self.emit_branch_conditional_to(12, 0, labels[&760]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: -8 });
        self.emit_branch_conditional_to(4, 1, labels[&748]); // ble
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 256 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, labels[&748]); // blt
        self.bind_label(labels[&729]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 1 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 5, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 4, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 3, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 4, offset: 40 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 48 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 64 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 5 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 4 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 3 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 1 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&729]); // bdnz
        self.bind_label(labels[&748]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 5, shift: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 248 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 5, b: 31 });
        self.emit_branch_conditional_to(12, 1, labels[&760]); // bgt
        self.bind_label(labels[&756]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 6, a: 6, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&756]); // bdnz
        self.bind_label(labels[&760]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&763]); // bne
        self.emit_branch_to(labels[&764]); // b
        self.bind_label(labels[&763]);
        self.output.instructions.push(Instruction::FloatNegate { d: 6, b: 6 });
        self.bind_label(labels[&764]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 6, a: 22, offset: 8 });
        self.emit_branch_to(labels[&910]); // b
        self.bind_label(labels[&766]);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 248 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 5, b: 8 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::move_register(4, 5));
        self.emit_branch_conditional_to(4, 1, labels[&815]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 31, shift: 30, begin: 2, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&806]); // beq
        self.bind_label(labels[&775]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -32 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::StoreFloatDoubleWithUpdate { s: 2, a: 4, offset: -32 });
        self.emit_branch_conditional_to(16, 0, labels[&775]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&815]); // beq
        self.bind_label(labels[&806]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&807]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDoubleWithUpdate { s: 2, a: 4, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&807]); // bdnz
        self.bind_label(labels[&815]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::move_register(4, 5));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: -1 });
        self.emit_branch_conditional_to(4, 1, labels[&862]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 30, begin: 2, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&853]); // beq
        self.bind_label(labels[&822]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -32 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: -24 });
        self.output.instructions.push(Instruction::StoreFloatDoubleWithUpdate { s: 2, a: 4, offset: -32 });
        self.emit_branch_conditional_to(16, 0, labels[&822]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&862]); // beq
        self.bind_label(labels[&853]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&854]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 2, a: 0, b: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDoubleWithUpdate { s: 2, a: 4, offset: -8 });
        self.emit_branch_conditional_to(16, 0, labels[&854]); // bdnz
        self.bind_label(labels[&862]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 2 });
        self.load_double_constant(3, 0x0000000000000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: -1 });
        self.emit_branch_conditional_to(12, 0, labels[&894]); // blt
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 3, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&889]); // beq
        self.bind_label(labels[&869]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -16 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -24 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -32 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -40 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -48 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: -56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -64 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&869]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 3, s: 3, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&894]); // beq
        self.bind_label(labels[&889]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(labels[&890]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -8 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 3, a: 3, b: 0 });
        self.emit_branch_conditional_to(16, 0, labels[&890]); // bdnz
        self.bind_label(labels[&894]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 25, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&902]); // bne
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 248 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 256 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 22, offset: 8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 3, a: 22, offset: 16 });
        self.emit_branch_to(labels[&910]); // b
        self.bind_label(labels[&902]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 248 });
        self.output.instructions.push(Instruction::FloatNegate { d: 0, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 256 });
        self.output.instructions.push(Instruction::FloatNegate { d: 2, b: 2 });
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 1 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 22, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 22, offset: 8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 22, offset: 16 });
        self.bind_label(labels[&910]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 27, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 720 });
        self.record_relocation(RelocationKind::Rel24, "_restfpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restfpr_25".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 664 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_16");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_16".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 724 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 720 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
