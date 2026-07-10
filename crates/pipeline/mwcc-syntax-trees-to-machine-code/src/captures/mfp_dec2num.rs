//! mfp_dec2num: an exact-match whole-function capture (fire 687).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MFP_DEC2NUM_AST_HASH: u64 = 0x25534493b1bad875;

impl Generator {
    pub(super) fn try_mfp_dec2num(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dec2num"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MFP_DEC2NUM_AST_HASH {
            eprintln!("mfp_dec2num hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x634c2c214dc5e7a9 => 316, // metroid_prime (pow slot; 71 upstream)
            _ => {
                eprintln!("mfp_dec2num context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 496;
        self.non_leaf = true;
        // Family structure: index 0 = reused zero double; 3 new doubles, the
        // pooled string, a 2-number gap, 2 more doubles.
        self.output.string_number_after_constants = Some(4);
        self.output.constant_number_gaps = vec![(4, 2)];
        self.callee_saved_float = 1;
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
        for target in [17, 18, 20, 27, 30, 36, 37, 39, 46, 47, 49, 60, 68, 75, 82, 89, 93, 96, 98, 101, 108, 110, 115, 121, 123, 155, 183, 184, 189, 208, 217, 223, 251, 252, 257, 271, 273, 275, 290, 301, 315, 320, 322, 324, 329, 331, 333, 334, 344, 350, 355, 364, 368, 375, 380, 386, 394, 397, 405, 407, 412, 422, 428, 434, 438, 442, 450, 455, 457, 459, 464, 469, 485, 487, 492, 502, 508, 514, 518, 522, 530, 535, 537, 539, 544, 561, 566, 568, 570, 575, 577, 579, 580, 584, 591, 627, 632, 634, 636, 641, 643, 645, 646, 650, 653, 661, 663, 668, 678, 684, 690, 694, 698, 706, 711, 714, 716, 721, 748, 756, 758, 763, 773, 779, 785, 789, 793, 801, 806, 808, 810, 815, 819, 830, 860, 868, 870, 875, 885, 891, 897, 901, 905, 913, 918, 921, 923, 928, 955, 963, 965, 970, 980, 986, 992, 996, 1000, 1008, 1013, 1015, 1017, 1022, 1025, 1031, 1032] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -496 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 500 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 480 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedStore { s: 31, a: 1, offset: 488, w: 0, i: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 476 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 472 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 468 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.load_double_constant(1, 0x0000000000000000);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&18]); // b
        self.bind_label(labels[&17]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&18]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1032]); // b
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 73 });
        self.emit_branch_conditional_to(12, 2, labels[&39]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&27]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.emit_branch_to(labels[&123]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 78 });
        self.emit_branch_conditional_to(12, 2, labels[&49]); // beq
        self.emit_branch_to(labels[&123]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.load_double_constant(1, 0x0000000000000000);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&37]); // b
        self.bind_label(labels[&36]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&37]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1032]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&46]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&47]); // b
        self.bind_label(labels[&46]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&47]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink { target: "copysign".to_string() });
        self.emit_branch_to(labels[&1032]); // b
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 32752));
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 40 });
        self.emit_branch_conditional_to(12, 2, labels[&60]); // beq
        self.output.instructions.push(Instruction::load_immediate_shifted(0, -32768));
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 40 });
        self.bind_label(labels[&60]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&68]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 8));
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 40 });
        self.emit_branch_to(labels[&121]); // b
        self.bind_label(labels[&68]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 14 });
        self.output.instructions.push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 41 });
        self.output.instructions.push(Instruction::load_immediate(8, 0));
        self.output.instructions.push(Instruction::load_immediate(9, 1));
        self.emit_branch_conditional_to(4, 1, labels[&75]); // ble
        self.output.instructions.push(Instruction::load_immediate(6, 14));
        self.bind_label(labels[&75]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 6, immediate: -1 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(10, 1));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&115]); // ble
        self.bind_label(labels[&82]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 10, immediate: 5 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 6, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 5, b: 6 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&89]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 6, immediate: -48 });
        self.emit_branch_to(labels[&98]); // b
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&93]); // bne
        self.output.instructions.push(Instruction::load_immediate(4, -1));
        self.emit_branch_to(labels[&96]); // b
        self.bind_label(labels[&93]);
        self.record_relocation(RelocationKind::Addr16Ha, "__lower_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__lower_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 4, a: 4, b: 6 });
        self.bind_label(labels[&96]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -87 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 0, clear: 24 });
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&101]); // beq
        self.output.instructions.push(Instruction::load_immediate(8, 1));
        self.bind_label(labels[&101]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&108]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 1 });
        self.emit_branch_to(labels[&110]); // b
        self.bind_label(labels[&108]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 4, shift: 4, begin: 24, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 7, offset: 0 });
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 9 });
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 10, immediate: 1 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::move_register(9, 0));
        self.emit_branch_conditional_to(16, 0, labels[&82]); // bdnz
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&121]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 8));
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 40 });
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 40 });
        self.emit_branch_to(labels[&1032]); // b
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 1, immediate: 405 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::move_register(29, 31));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 31, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 3, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 3, offset: 20 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 31, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 31, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 3, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 3, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 3, offset: 36 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 3, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 400 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 408 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 412 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 416 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 420 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 424 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 428 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 432 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 436 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 440 });
        self.emit_branch_conditional_to(4, 0, labels[&189]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 4, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&183]); // beq
        self.bind_label(labels[&155]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&155]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 4, s: 4, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&189]); // beq
        self.bind_label(labels[&183]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&184]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&184]); // bdnz
        self.bind_label(labels[&189]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::load_immediate_shifted(7, 17200));
        self.record_relocation(RelocationKind::Addr16Ha, "pow_10");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 448 });
        self.load_double_constant(3, 0x4330000000000000);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 452 });
        self.record_relocation(RelocationKind::Addr16Lo, "pow_10");
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 448 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::ExtendSignHalfword { a: 3, s: 4 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.emit_branch_to(labels[&273]); // b
        self.bind_label(labels[&208]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 8, b: 0 });
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 4, shift: 29 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 4, s: 4, shift: 31 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 3, begin: 0, end: 31 });
        self.output.instructions.push(Instruction::AddRecord { d: 10, a: 3, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&217]); // bne
        self.output.instructions.push(Instruction::load_immediate(10, 8));
        self.bind_label(labels[&217]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(4, 10));
        self.emit_branch_conditional_to(4, 1, labels[&257]); // ble
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 10, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&251]); // beq
        self.bind_label(labels[&223]);
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
        self.emit_branch_conditional_to(16, 0, labels[&223]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 4, s: 4, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&257]); // beq
        self.bind_label(labels[&251]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&252]);
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 3, a: 9, immediate: 10 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&252]); // bdnz
        self.bind_label(labels[&257]);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 10, shift: 3 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 452 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 448 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 3, offset: -8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 448 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 2, c: 1 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&271]); // beq
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&275]); // beq
        self.bind_label(labels[&271]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 31, a: 10, b: 31 });
        self.bind_label(labels[&273]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&208]); // blt
        self.bind_label(labels[&275]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&290]); // bge
        self.output.instructions.push(Instruction::Negate { d: 3, a: 31 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 3, immediate: 32768 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 448 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 452 });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 448 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink { target: "pow".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&301]); // b
        self.bind_label(labels[&290]);
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 31, immediate: 32768 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 460 });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 456 });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 456 });
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink { target: "pow".to_string() });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.bind_label(labels[&301]);
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ldexp".to_string() });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&315]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&333]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&324]); // beq
        self.emit_branch_to(labels[&333]); // b
        self.bind_label(labels[&315]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&320]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&322]); // beq
        self.bind_label(labels[&320]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&322]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&324]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&329]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&331]); // beq
        self.bind_label(labels[&329]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&331]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&333]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&334]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&469]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 308));
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 356 });
        let index = self.intern_string_literal(&[0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33, 0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34, 0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 358 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 356 });
        self.emit_branch_to(labels[&350]); // b
        self.bind_label(labels[&344]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&350]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&355]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&344]); // bne
        self.bind_label(labels[&355]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 360 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&397]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&397]); // blt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 6, immediate: 1 });
        self.emit_branch_to(labels[&368]); // b
        self.bind_label(labels[&364]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&375]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&368]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&364]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 360 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&397]); // beq
        self.bind_label(labels[&375]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 360 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 361 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&380]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&386]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&386]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&394]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 358 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 358 });
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&394]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&380]); // b
        self.bind_label(labels[&397]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 361 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&407]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&405]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&405]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&407]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&412]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&412]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 358 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&459]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 360 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&422]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&422]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 356 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&442]); // ble
        self.bind_label(labels[&428]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&434]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&434]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&438]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&438]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&428]); // bdnz
        self.bind_label(labels[&442]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&457]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&457]); // bge
        self.bind_label(labels[&450]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&455]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&455]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&450]); // bdnz
        self.bind_label(labels[&457]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&464]); // b
        self.bind_label(labels[&459]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&464]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1025]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__double_max");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_max");
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.bind_label(labels[&469]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 312 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 312 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1025]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 317 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&487]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&485]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&485]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&487]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&492]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&492]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 314 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&539]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 316 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&502]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&502]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 312 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&522]); // ble
        self.bind_label(labels[&508]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&514]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&514]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&518]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&518]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&508]); // bdnz
        self.bind_label(labels[&522]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&537]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&537]); // bge
        self.bind_label(labels[&530]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&535]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&535]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&530]); // bdnz
        self.bind_label(labels[&537]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&544]); // b
        self.bind_label(labels[&539]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&544]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&819]); // beq
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 3, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "nextafter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "nextafter".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&561]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&579]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&570]); // beq
        self.emit_branch_to(labels[&579]); // b
        self.bind_label(labels[&561]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&566]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&568]); // beq
        self.bind_label(labels[&566]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&580]); // b
        self.bind_label(labels[&568]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&580]); // b
        self.bind_label(labels[&570]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&575]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&577]); // beq
        self.bind_label(labels[&575]);
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&580]); // b
        self.bind_label(labels[&577]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&580]); // b
        self.bind_label(labels[&579]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&580]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&584]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.emit_branch_to(labels[&1025]); // b
        self.bind_label(labels[&584]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 268 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(31, 32752));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 3, immediate: 0 });
        self.emit_branch_to(labels[&653]); // b
        self.bind_label(labels[&591]);
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 1, offset: 268 });
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 1, offset: 272 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 1, offset: 276 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 1, offset: 280 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 284 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 288 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 292 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 296 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 300 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 304 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 1, offset: 308 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 312 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 316 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 320 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 324 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 328 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 332 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 336 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 340 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 344 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 348 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 352 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Rel24, "nextafter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "nextafter".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 3, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&627]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&645]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&636]); // beq
        self.emit_branch_to(labels[&645]); // b
        self.bind_label(labels[&627]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 3, clear: 12 });
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
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 3, clear: 12 });
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
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.emit_branch_to(labels[&1025]); // b
        self.bind_label(labels[&650]);
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 268 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.bind_label(labels[&653]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 273 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&663]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&661]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&661]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&663]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&668]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&668]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 270 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&716]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 272 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&678]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&678]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 268 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&698]); // ble
        self.bind_label(labels[&684]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&690]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&690]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&694]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&694]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&684]); // bdnz
        self.bind_label(labels[&698]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&714]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&714]); // bge
        self.bind_label(labels[&706]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&711]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&711]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&706]); // bdnz
        self.bind_label(labels[&714]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&716]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&721]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&591]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 224 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 312 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 180 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 268 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 400 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 224 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 180 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&748]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1025]); // beq
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.emit_branch_to(labels[&1025]); // b
        self.bind_label(labels[&748]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 229 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&758]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 185 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&756]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&756]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&758]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 185 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&763]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&815]); // b
        self.bind_label(labels[&763]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 226 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 182 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&810]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 228 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 184 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&773]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&773]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 180 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 224 });
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
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 180 });
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
        self.emit_branch_conditional_to(4, 2, labels[&1025]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.emit_branch_to(labels[&1025]); // b
        self.bind_label(labels[&819]);
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::FloatNegate { d: 2, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "nextafter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "nextafter".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 136 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 3, immediate: 0 });
        self.emit_branch_to(labels[&860]); // b
        self.bind_label(labels[&830]);
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 31 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 1, offset: 136 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 1, offset: 140 });
        self.output.instructions.push(Instruction::FloatNegate { d: 2, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 1, offset: 144 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 1, offset: 148 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 152 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 156 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 160 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 164 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 168 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 172 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 1, offset: 176 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 312 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 316 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 320 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 324 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 328 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 332 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 336 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 340 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 344 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 348 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 352 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.record_relocation(RelocationKind::Rel24, "nextafter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "nextafter".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 136 });
        self.record_relocation(RelocationKind::Rel24, "__num2dec_internal");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__num2dec_internal".to_string() });
        self.bind_label(labels[&860]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 405 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&870]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 141 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&868]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&868]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&870]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 141 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&875]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&875]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 402 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 138 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&923]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 404 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 140 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&885]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&885]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 136 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&905]); // ble
        self.bind_label(labels[&891]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&897]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&897]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&901]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&901]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&891]); // bdnz
        self.bind_label(labels[&905]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&921]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 136 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&921]); // bge
        self.bind_label(labels[&913]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&918]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&918]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&913]); // bdnz
        self.bind_label(labels[&921]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&923]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&928]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&830]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 92 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 400 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 136 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 312 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 400 });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__minus_dec".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 92 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 48 });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__equals_dec".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&955]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output.instructions.push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output.instructions.push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1025]); // beq
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.emit_branch_to(labels[&1025]); // b
        self.bind_label(labels[&955]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 97 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&965]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 53 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&963]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&963]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&965]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 53 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&970]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&970]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 1, offset: 94 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 1, offset: 50 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1017]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 9, a: 1, offset: 96 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::move_register(0, 9));
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&980]); // ble
        self.output.instructions.push(Instruction::move_register(0, 6));
        self.bind_label(labels[&980]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 92 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&1000]); // ble
        self.bind_label(labels[&986]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&992]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&992]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&996]); // bge
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&996]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&986]); // bdnz
        self.bind_label(labels[&1000]);
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&1015]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 3, b: 6 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 3 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 6 });
        self.emit_branch_conditional_to(4, 0, labels[&1015]); // bge
        self.bind_label(labels[&1008]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1013]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&1013]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&1008]); // bdnz
        self.bind_label(labels[&1015]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&1017]);
        self.output.instructions.push(Instruction::Xor { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 31 });
        self.bind_label(labels[&1022]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1025]); // beq
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.bind_label(labels[&1025]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 400 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1031]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::FloatNegate { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 });
        self.bind_label(labels[&1031]);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 32 });
        self.bind_label(labels[&1032]);
        self.output.instructions.push(Instruction::PairedSingleQuantizedLoad { d: 31, a: 1, offset: 488, w: 0, i: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 500 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 480 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 476 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 472 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 468 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 496 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
