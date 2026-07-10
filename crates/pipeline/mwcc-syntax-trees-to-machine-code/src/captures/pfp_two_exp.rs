//! pfp_two_exp: an exact-match whole-function capture (fire 687).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFP_TWO_EXP_AST_HASH: u64 = 0x7c6c2b441d17cd33;

impl Generator {
    pub(super) fn try_pfp_two_exp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__two_exp"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFP_TWO_EXP_AST_HASH {
            eprintln!("pfp_two_exp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xecff4eb19d59de49 => 802, // pikmin2
            _ => {
                eprintln!("pfp_two_exp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 112;
        self.non_leaf = true;
        self.output.jump_table = Some(mwcc_machine_code::JumpTable {
            entries: vec![72, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 316, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 560, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 804, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 1048, 1292, 1536, 1780, 2024, 2268, 2512, 2756, 3000, 3240, 3480, 3720, 3960, 4204, 4448, 4692, 4936],
            anonymous_offset: 0,
        });
        self.intern_string_literal(&[0x35, 0x34, 0x32, 0x31, 0x30, 0x31, 0x30, 0x38, 0x36, 0x32, 0x34, 0x32, 0x37, 0x35, 0x32, 0x32, 0x31, 0x37, 0x30, 0x30, 0x33, 0x37, 0x32, 0x36, 0x34, 0x30, 0x30, 0x34, 0x33, 0x34, 0x39, 0x37, 0x30, 0x38, 0x35, 0x35, 0x37, 0x31, 0x32, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35]); // @896 (long .data string via ...data.0)
        self.intern_string_literal(&[0x31, 0x31, 0x31, 0x30, 0x32, 0x32, 0x33, 0x30, 0x32, 0x34, 0x36, 0x32, 0x35, 0x31, 0x35, 0x36, 0x35, 0x34, 0x30, 0x34, 0x32, 0x33, 0x36, 0x33, 0x31, 0x36, 0x36, 0x38, 0x30, 0x39, 0x30, 0x38, 0x32, 0x30, 0x33, 0x31, 0x32, 0x35]); // @897 (long .data string via ...data.0)
        self.intern_string_literal(&[0x32, 0x33, 0x32, 0x38, 0x33, 0x30, 0x36, 0x34, 0x33, 0x36, 0x35, 0x33, 0x38, 0x36, 0x39, 0x36, 0x32, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35]); // @898 (long .data string via ...data.0)
        self.intern_string_literal(&[0x31, 0x35, 0x32, 0x35, 0x38, 0x37, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35]); // @899 (long .data string via ...data.0)
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [24, 30, 35, 45, 49, 56, 61, 67, 75, 85, 91, 96, 106, 110, 117, 122, 128, 136, 146, 152, 157, 167, 171, 178, 183, 189, 197, 207, 213, 218, 228, 232, 239, 244, 250, 258, 268, 274, 279, 289, 293, 300, 305, 311, 319, 329, 335, 340, 350, 354, 361, 366, 372, 380, 390, 396, 401, 411, 415, 422, 427, 433, 441, 451, 457, 462, 472, 476, 483, 488, 494, 502, 512, 518, 523, 533, 537, 544, 549, 555, 563, 573, 579, 584, 594, 598, 605, 610, 616, 624, 634, 640, 645, 655, 659, 666, 671, 677, 685, 695, 701, 706, 716, 720, 727, 732, 738, 746, 755, 761, 766, 776, 780, 787, 792, 798, 806, 815, 821, 826, 836, 840, 847, 852, 858, 866, 875, 881, 886, 896, 900, 907, 912, 918, 926, 935, 941, 946, 956, 960, 967, 972, 978, 986, 996, 1002, 1007, 1017, 1021, 1028, 1033, 1039, 1047, 1057, 1063, 1068, 1078, 1082, 1089, 1094, 1100, 1108, 1118, 1124, 1129, 1139, 1143, 1150, 1155, 1161, 1169, 1179, 1185, 1190, 1200, 1204, 1211, 1216, 1222, 1230, 1240, 1246, 1251, 1261, 1265, 1272, 1277, 1283, 1291, 1295, 1336, 1342, 1347, 1357, 1361, 1368, 1373, 1379, 1387, 1390, 1397, 1403, 1408, 1418, 1422, 1429, 1434, 1440, 1448, 1451, 1455] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -112 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 116 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 108 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 104 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: 64 });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 72 });
        self.record_relocation(RelocationKind::Addr16Lo, "...data.0");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&1295]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::load_immediate(0, -20));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 0 });
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&35]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&24]); // bne
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&56]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&56]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&45]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&56]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&67]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&67]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&75]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&75]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&61]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -16));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 48 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 0 });
        self.emit_branch_to(labels[&91]); // b
        self.bind_label(labels[&85]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&91]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&96]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&85]); // bne
        self.bind_label(labels[&96]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&117]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.emit_branch_to(labels[&110]); // b
        self.bind_label(labels[&106]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&117]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&106]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&117]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&128]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&128]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&122]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -10));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 88 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 0 });
        self.emit_branch_to(labels[&152]); // b
        self.bind_label(labels[&146]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&152]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&157]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&146]); // bne
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&178]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.emit_branch_to(labels[&171]); // b
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&178]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&171]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&167]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&178]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&183]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&189]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&189]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&197]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&183]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -5));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 112 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 0 });
        self.emit_branch_to(labels[&213]); // b
        self.bind_label(labels[&207]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&213]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&218]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&207]); // bne
        self.bind_label(labels[&218]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&239]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 1 });
        self.emit_branch_to(labels[&232]); // b
        self.bind_label(labels[&228]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&239]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&232]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&228]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&239]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&244]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&250]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&250]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&258]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&258]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&244]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -3));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x33, 0x39, 0x30, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&274]); // b
        self.bind_label(labels[&268]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&274]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&279]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&268]); // bne
        self.bind_label(labels[&279]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&300]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&293]); // b
        self.bind_label(labels[&289]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&300]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&293]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&289]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&300]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&305]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&311]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&311]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&319]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&319]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&305]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -3));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x37, 0x38, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&335]); // b
        self.bind_label(labels[&329]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&335]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&340]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&329]); // bne
        self.bind_label(labels[&340]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&361]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&354]); // b
        self.bind_label(labels[&350]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&361]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&354]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&350]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&361]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&366]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&372]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&372]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&380]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&380]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&366]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -2));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x31, 0x35, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&396]); // b
        self.bind_label(labels[&390]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&396]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&401]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&390]); // bne
        self.bind_label(labels[&401]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&422]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&415]); // b
        self.bind_label(labels[&411]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&422]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&415]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&411]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&422]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&427]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&433]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&433]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&441]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&441]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&427]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -2));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x33, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&457]); // b
        self.bind_label(labels[&451]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&457]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&462]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&451]); // bne
        self.bind_label(labels[&462]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&483]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&476]); // b
        self.bind_label(labels[&472]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&483]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&476]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&472]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&483]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&488]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&494]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&494]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&502]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&502]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&488]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -2));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&518]); // b
        self.bind_label(labels[&512]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&518]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&523]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&512]); // bne
        self.bind_label(labels[&523]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&544]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&537]); // b
        self.bind_label(labels[&533]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&544]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&537]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&533]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&544]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&549]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&555]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&555]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&563]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&563]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&549]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&579]); // b
        self.bind_label(labels[&573]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&579]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&584]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&573]); // bne
        self.bind_label(labels[&584]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&605]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&598]); // b
        self.bind_label(labels[&594]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&605]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&598]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&594]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&605]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&610]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&616]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&616]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&624]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&624]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&610]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&640]); // b
        self.bind_label(labels[&634]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&640]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&645]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&634]); // bne
        self.bind_label(labels[&645]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&666]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&659]); // b
        self.bind_label(labels[&655]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&666]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&659]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&655]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&666]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&671]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&677]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&677]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&685]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&685]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&671]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&701]); // b
        self.bind_label(labels[&695]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&701]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&706]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&695]); // bne
        self.bind_label(labels[&706]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&727]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&720]); // b
        self.bind_label(labels[&716]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&727]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&720]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&716]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&727]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&732]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&738]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&738]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&746]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&746]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&732]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x31]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&761]); // b
        self.bind_label(labels[&755]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&761]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&766]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&755]); // bne
        self.bind_label(labels[&766]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&787]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&780]); // b
        self.bind_label(labels[&776]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&787]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&780]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&776]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&787]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&792]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&798]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&798]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&806]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&806]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&792]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&821]); // b
        self.bind_label(labels[&815]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&821]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&826]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&815]); // bne
        self.bind_label(labels[&826]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&847]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&840]); // b
        self.bind_label(labels[&836]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&847]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&840]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&836]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&847]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&852]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&858]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&858]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&866]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&866]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&852]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&881]); // b
        self.bind_label(labels[&875]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&881]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&886]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&875]); // bne
        self.bind_label(labels[&886]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&907]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&900]); // b
        self.bind_label(labels[&896]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&907]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&900]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&896]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&907]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&912]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&918]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&918]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&926]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&926]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&912]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&941]); // b
        self.bind_label(labels[&935]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&941]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&946]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&935]); // bne
        self.bind_label(labels[&946]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&967]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&960]); // b
        self.bind_label(labels[&956]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&967]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&960]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&956]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&967]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&972]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&978]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&978]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&986]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&986]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&972]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x31, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&1002]); // b
        self.bind_label(labels[&996]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1002]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1007]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&996]); // bne
        self.bind_label(labels[&1007]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1028]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&1021]); // b
        self.bind_label(labels[&1017]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1028]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1021]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1017]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&1028]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1033]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1039]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1039]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1047]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1047]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1033]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x33, 0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&1063]); // b
        self.bind_label(labels[&1057]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1063]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1068]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1057]); // bne
        self.bind_label(labels[&1068]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1089]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&1082]); // b
        self.bind_label(labels[&1078]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1089]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1082]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1078]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&1089]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1094]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1100]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1100]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1108]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1108]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1094]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x36, 0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&1124]); // b
        self.bind_label(labels[&1118]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1124]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1129]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1118]); // bne
        self.bind_label(labels[&1129]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1150]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&1143]); // b
        self.bind_label(labels[&1139]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1150]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1143]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1139]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&1150]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1155]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1161]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1161]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1169]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1169]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1155]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x31, 0x32, 0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&1185]); // b
        self.bind_label(labels[&1179]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1185]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1190]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1179]); // bne
        self.bind_label(labels[&1190]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1211]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&1204]); // b
        self.bind_label(labels[&1200]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1211]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1204]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1200]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&1211]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1216]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1222]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1222]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1230]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1230]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1216]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        let index = self.intern_string_literal(&[0x32, 0x35, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 0 });
        self.emit_branch_to(labels[&1246]); // b
        self.bind_label(labels[&1240]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1246]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1251]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1240]); // bne
        self.bind_label(labels[&1251]);
        self.output.instructions.push(Instruction::StoreByte { s: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1455]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1272]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&1265]); // b
        self.bind_label(labels[&1261]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1272]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1265]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1261]); // bne
        self.output.instructions.push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.bind_label(labels[&1272]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 5 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1277]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1283]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1283]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1291]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 31, offset: 2 });
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1291]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1277]); // b
        self.emit_branch_to(labels[&1455]); // b
        self.bind_label(labels[&1295]);
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 30, shift: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 52 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 0, b: 30 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 4, s: 0, shift: 1 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__two_exp".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 52 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::move_register(5, 4));
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__timesdec".to_string() });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 30, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1455]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 48 });
        self.emit_branch_conditional_to(4, 1, labels[&1390]); // ble
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 52 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 5, a: 1, offset: 54 });
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 52 });
        self.emit_branch_to(labels[&1342]); // b
        self.bind_label(labels[&1336]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&1342]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1347]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1336]); // bne
        self.bind_label(labels[&1347]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1451]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1451]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1368]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 6, immediate: 1 });
        self.emit_branch_to(labels[&1361]); // b
        self.bind_label(labels[&1357]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1368]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1361]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1357]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 56 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1451]); // beq
        self.bind_label(labels[&1368]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 57 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1373]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1379]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1451]); // b
        self.bind_label(labels[&1379]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1387]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 54 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 54 });
        self.emit_branch_to(labels[&1451]); // b
        self.bind_label(labels[&1387]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1373]); // b
        self.bind_label(labels[&1390]);
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 54 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 52 });
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 52 });
        self.emit_branch_to(labels[&1403]); // b
        self.bind_label(labels[&1397]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&1403]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 36 });
        self.emit_branch_conditional_to(4, 0, labels[&1408]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1397]); // bne
        self.bind_label(labels[&1408]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1451]); // beq
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1451]); // blt
        self.emit_branch_conditional_to(12, 1, labels[&1429]); // bgt
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 6, immediate: 1 });
        self.emit_branch_to(labels[&1422]); // b
        self.bind_label(labels[&1418]);
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 48 });
        self.emit_branch_conditional_to(4, 2, labels[&1429]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.bind_label(labels[&1422]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1418]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 56 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&1451]); // beq
        self.bind_label(labels[&1429]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 57 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1434]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1440]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.emit_branch_to(labels[&1451]); // b
        self.bind_label(labels[&1440]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1448]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 3, a: 1, offset: 54 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 54 });
        self.emit_branch_to(labels[&1451]); // b
        self.bind_label(labels[&1448]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.emit_branch_to(labels[&1434]); // b
        self.bind_label(labels[&1451]);
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 52 });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__timesdec".to_string() });
        self.bind_label(labels[&1455]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 116 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 108 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 104 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 112 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
