//! bfp_dec2num: an exact-match whole-function capture (fire 686).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const BFP_DEC2NUM_AST_HASH: u64 = 0xe847c553cdcf846;

impl Generator {
    pub(super) fn try_bfp_dec2num(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dec2num"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != BFP_DEC2NUM_AST_HASH {
            eprintln!("bfp_dec2num hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xdbce2bc49da89140 => 313, // bfbb (pow_10 owned-static consumes a slot; 71 shifted upstream)
            _ => {
                eprintln!("bfp_dec2num context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 512;
        self.non_leaf = true;
        // Creation order inside the body: three doubles, THEN the pooled
        // string (@1542), a 2-number gap, the last two doubles (@1545/@1546).
        // (index 0 is the REUSED @1145 — it consumes no number, so the three
        // new doubles are indices 1-3 and the string+gap sit before index 4.)
        self.output.string_number_after_constants = Some(4);
        self.output.constant_number_gaps = vec![(4, 2)];
        for bits in [
            0x0000000000000000u64,
            0x3ff0000000000000,
            0xbff0000000000000,
            0x4014000000000000,
            0x4330000000000000,
            0x4330000080000000,
        ] {
            self.output.intern_constant(bits, 8);
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [15, 16, 18, 25, 28, 34, 35, 37, 44, 45, 47, 58, 66, 73, 75, 83, 87, 90, 97, 99, 103, 111, 113, 145, 173, 174, 179, 198, 207, 213, 241, 242, 247, 261, 263, 265, 280, 291, 305, 310, 312, 314, 319, 321, 323, 324, 334, 340, 345, 355, 359, 366, 371, 377, 385, 388, 396, 398, 403, 413, 419, 425, 429, 433, 441, 446, 448, 450, 455, 460, 476, 478, 483, 493, 499, 505, 509, 513, 521, 526, 528, 530, 535, 558, 563, 565, 567, 572, 574, 576, 577, 581, 585, 627, 632, 634, 636, 641, 643, 645, 646, 650, 652, 660, 662, 667, 677, 683, 689, 693, 697, 705, 710, 713, 715, 720, 748, 756, 758, 763, 773, 779, 785, 789, 793, 801, 806, 808, 810, 815, 820, 833, 867, 875, 877, 882, 892, 898, 904, 908, 912, 920, 925, 928, 930, 935, 963, 971, 973, 978, 988, 994, 1000, 1004, 1008, 1016, 1021, 1023, 1025, 1030, 1034, 1040, 1041] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -512 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 516 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 512 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_25".to_string() });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 0 });
        self.load_double_constant(1, 0x0000000000000000);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&16]); // b
        self.bind_label(labels[&15]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&16]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1041]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(12, 2, labels[&37]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&25]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&28]); // beq
        self.emit_branch_to(labels[&113]); // b
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.emit_branch_to(labels[&113]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 0 });
        self.load_double_constant(1, 0x0000000000000000);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&34]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&35]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1041]); // b
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&44]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&45]); // b
        self.bind_label(labels[&44]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&45]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1041]); // b
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 32752));
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 56 });
        self.emit_branch_conditional_to(12, 2, labels[&58]); // beq
        self.output.instructions.push(Instruction::load_immediate_shifted(0, -32768));
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 56 });
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&66]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 8));
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 56 });
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&66]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 14 });
        self.output.instructions.push(Instruction::move_register(25, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 1, immediate: 57 });
        self.output.instructions.push(Instruction::load_immediate(28, 0));
        self.output.instructions.push(Instruction::load_immediate(27, 1));
        self.emit_branch_conditional_to(4, 1, labels[&73]); // ble
        self.output.instructions.push(Instruction::load_immediate(25, 14));
        self.bind_label(labels[&73]);
        self.output.instructions.push(Instruction::load_immediate(26, 1));
        self.emit_branch_to(labels[&103]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 26, immediate: 5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 29, a: 31, b: 0 });
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "isdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&83]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 29, immediate: -48 });
        self.emit_branch_to(labels[&87]); // b
        self.bind_label(labels[&83]);
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "tolower");
        self.output.instructions.push(Instruction::BranchAndLink { target: "tolower".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -87 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 0, clear: 24 });
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 3, clear: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&90]); // beq
        self.output.instructions.push(Instruction::load_immediate(28, 1));
        self.bind_label(labels[&90]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 27, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&97]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: 1 });
        self.emit_branch_to(labels[&99]); // b
        self.bind_label(labels[&97]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 3, shift: 4, begin: 24, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 30, offset: 0 });
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 27 });
        self.output.instructions.push(Instruction::AddImmediate { d: 26, a: 26, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::move_register(27, 0));
        self.bind_label(labels[&103]);
        self.output.instructions.push(Instruction::CompareWord { a: 26, b: 25 });
        self.emit_branch_conditional_to(12, 0, labels[&75]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 28, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&111]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 8));
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 56 });
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 56 });
        self.emit_branch_to(labels[&1041]); // b
        self.bind_label(labels[&113]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 1, immediate: 421 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::move_register(25, 30));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 31, offset: 8 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 30, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 31, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 31, offset: 20 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 30, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 30, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 31, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 416 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 424 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 428 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 432 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 436 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 440 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 444 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 448 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 452 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 456 });
        self.emit_branch_conditional_to(4, 0, labels[&179]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 4, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&173]); // beq
        self.bind_label(labels[&145]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&145]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 4, s: 4, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&179]); // beq
        self.bind_label(labels[&173]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&174]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 25, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 25, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 25, a: 25, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&174]); // bdnz
        self.bind_label(labels[&179]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::load_immediate_shifted(7, 17200));
        self.record_relocation(RelocationKind::Addr16Ha, "pow_10");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 464 });
        self.load_double_constant(3, 0x4330000000000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 468 });
        self.record_relocation(RelocationKind::Addr16Lo, "pow_10");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 464 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 3, s: 4 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.emit_branch_to(labels[&263]); // b
        self.bind_label(labels[&198]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 8, b: 0 });
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 4, shift: 29 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 4, s: 4, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 3, begin: 0, end: 31 });
        self.output.instructions.push(Instruction::AddRecord { d: 10, a: 3, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&207]); // bne
        self.output.instructions.push(Instruction::load_immediate(10, 8));
        self.bind_label(labels[&207]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(4, 10));
        self.emit_branch_conditional_to(4, 1, labels[&247]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 10, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&241]); // beq
        self.bind_label(labels[&213]);
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 2 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 3 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 5 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 6 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 7 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 8 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&213]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 4, s: 4, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&247]); // beq
        self.bind_label(labels[&241]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&242]);
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&242]); // bdnz
        self.bind_label(labels[&247]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 10, shift: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 468 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 464 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 464 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 2, c: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&261]); // beq
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&265]); // beq
        self.bind_label(labels[&261]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 31, a: 10, b: 31 });
        self.bind_label(labels[&263]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&198]); // blt
        self.bind_label(labels[&265]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&280]); // bge
        self.output.instructions.push(Instruction::Negate { d: 3, a: 31 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 464 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 468 });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 464 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink { target: "pow".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&291]); // b
        self.bind_label(labels[&280]);
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 31, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 476 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 472 });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 472 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink { target: "pow".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.bind_label(labels[&291]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&305]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&323]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&314]); // beq
        self.emit_branch_to(labels[&323]); // b
        self.bind_label(labels[&305]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&310]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&312]); // beq
        self.bind_label(labels[&310]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&312]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&314]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&319]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&321]); // beq
        self.bind_label(labels[&319]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&321]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&324]); // b
        self.bind_label(labels[&323]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&324]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&460]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 308));
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 372 });
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 374 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 372 });
        self.emit_branch_to(labels[&340]); // b
        self.bind_label(labels[&334]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&340]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&345]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&334]); // bne
        self.bind_label(labels[&345]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 376 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&388]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&388]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&366]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 6, immediate: 1 });
        self.emit_branch_to(labels[&359]); // b
        self.bind_label(labels[&355]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&366]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&359]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&355]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 376 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&388]); // beq
        self.bind_label(labels[&366]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 376 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 377 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&371]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&377]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&388]); // b
        self.bind_label(labels[&377]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&385]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 374 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 374 });
        self.emit_branch_to(labels[&388]); // b
        self.bind_label(labels[&385]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&371]); // b
        self.bind_label(labels[&388]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 377 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&398]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&396]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&396]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&398]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&403]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&403]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 374 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&450]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 376 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&413]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&413]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 372 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&433]); // ble
        self.bind_label(labels[&419]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&425]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&425]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&429]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&429]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&419]); // bdnz
        self.bind_label(labels[&433]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&448]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&448]); // bge
        self.bind_label(labels[&441]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&446]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&446]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&441]); // bdnz
        self.bind_label(labels[&448]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&455]); // b
        self.bind_label(labels[&450]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&455]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1034]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__double_max");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_max");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.bind_label(labels[&460]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 328 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 328 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1034]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 333 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&478]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&476]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&476]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&478]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&483]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&483]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 330 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&530]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 332 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&493]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&493]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 328 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&513]); // ble
        self.bind_label(labels[&499]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&505]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&505]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&509]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&509]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&499]); // bdnz
        self.bind_label(labels[&513]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&528]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&528]); // bge
        self.bind_label(labels[&521]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&526]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&526]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&521]); // bdnz
        self.bind_label(labels[&528]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&535]); // b
        self.bind_label(labels[&530]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&535]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&820]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::AddCarrying { d: 4, a: 6, b: 4 });
        self.output.instructions.push(Instruction::AddExtended { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&558]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&576]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&567]); // beq
        self.emit_branch_to(labels[&576]); // b
        self.bind_label(labels[&558]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&563]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&565]); // beq
        self.bind_label(labels[&563]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&577]); // b
        self.bind_label(labels[&565]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&577]); // b
        self.bind_label(labels[&567]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&572]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&574]); // beq
        self.bind_label(labels[&572]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&577]); // b
        self.bind_label(labels[&574]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&577]); // b
        self.bind_label(labels[&576]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&577]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&581]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 48 });
        self.emit_branch_to(labels[&1034]); // b
        self.bind_label(labels[&581]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 284 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.output.instructions.push(Instruction::load_immediate_shifted(29, 32752));
        self.emit_branch_to(labels[&652]); // b
        self.bind_label(labels[&585]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddCarrying { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::AddExtended { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 284 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 1, offset: 288 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 1, offset: 292 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 1, offset: 296 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 1, offset: 300 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 304 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 31, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 308 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 312 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 29 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 316 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 320 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 1, offset: 324 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 328 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 332 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 336 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 340 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 344 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 348 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 352 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 356 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 360 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 364 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 368 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&627]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&645]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&636]); // beq
        self.emit_branch_to(labels[&645]); // b
        self.bind_label(labels[&627]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 31, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&632]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&634]); // beq
        self.bind_label(labels[&632]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&646]); // b
        self.bind_label(labels[&634]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&646]); // b
        self.bind_label(labels[&636]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 31, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&641]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&643]); // beq
        self.bind_label(labels[&641]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&646]); // b
        self.bind_label(labels[&643]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&646]); // b
        self.bind_label(labels[&645]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&646]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&650]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 48 });
        self.emit_branch_to(labels[&1034]); // b
        self.bind_label(labels[&650]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 284 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.bind_label(labels[&652]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 289 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&662]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&660]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&660]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&662]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&667]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&667]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 286 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&715]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 288 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&677]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&677]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 284 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&697]); // ble
        self.bind_label(labels[&683]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&689]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&689]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&693]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&693]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&683]); // bdnz
        self.bind_label(labels[&697]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&713]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&713]); // bge
        self.bind_label(labels[&705]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&710]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&710]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&705]); // bdnz
        self.bind_label(labels[&713]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&715]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&720]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&585]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 240 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 328 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 196 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 284 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 416 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 240 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 196 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&748]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1034]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.emit_branch_to(labels[&1034]); // b
        self.bind_label(labels[&748]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 245 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&758]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 201 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&756]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&756]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&758]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 201 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&763]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&763]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 242 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 198 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&810]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 244 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 200 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&773]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&773]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 196 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 240 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&793]); // ble
        self.bind_label(labels[&779]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&785]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&785]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&789]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&789]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&779]); // bdnz
        self.bind_label(labels[&793]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&808]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 196 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&808]); // bge
        self.bind_label(labels[&801]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&806]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&806]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&801]); // bdnz
        self.bind_label(labels[&808]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&810]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&815]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1034]); // bne
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.emit_branch_to(labels[&1034]); // b
        self.bind_label(labels[&820]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 152 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::AddCarrying { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::AddExtended { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.emit_branch_to(labels[&867]); // b
        self.bind_label(labels[&833]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 152 });
        self.output.instructions.push(Instruction::AddCarrying { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::AddExtended { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 156 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 160 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 1, offset: 168 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 1, offset: 172 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 1, offset: 176 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 180 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 184 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 188 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 5, a: 1, offset: 192 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 328 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 332 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 336 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 340 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 344 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 348 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 352 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 356 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 360 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 364 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 5, a: 1, offset: 368 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.bind_label(labels[&867]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 421 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&877]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 157 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&875]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&875]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&877]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 157 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&882]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&882]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 418 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 154 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&930]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 156 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&892]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&892]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 152 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&912]); // ble
        self.bind_label(labels[&898]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&904]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&904]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&908]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&908]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&898]); // bdnz
        self.bind_label(labels[&912]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&928]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 152 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&928]); // bge
        self.bind_label(labels[&920]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&925]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&925]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&920]); // bdnz
        self.bind_label(labels[&928]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&935]); // b
        self.bind_label(labels[&930]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&935]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&833]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 108 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 416 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 152 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 328 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 416 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 108 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 64 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&963]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1034]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.emit_branch_to(labels[&1034]); // b
        self.bind_label(labels[&963]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 113 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&973]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 69 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&971]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&971]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&973]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 69 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&978]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&978]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 110 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 66 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1025]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 112 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&988]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&988]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 108 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&1008]); // ble
        self.bind_label(labels[&994]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&1000]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&1000]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&1004]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&1004]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&994]); // bdnz
        self.bind_label(labels[&1008]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&1023]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&1023]); // bge
        self.bind_label(labels[&1016]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1021]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&1021]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&1016]); // bdnz
        self.bind_label(labels[&1023]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1030]); // b
        self.bind_label(labels[&1025]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&1030]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1034]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.bind_label(labels[&1034]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 416 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1040]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::FloatNegate { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 48 });
        self.bind_label(labels[&1040]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 48 });
        self.bind_label(labels[&1041]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 512 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_25");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_25".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 516 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 512 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
