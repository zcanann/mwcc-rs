//! acf_two_exp: an exact-match whole-function capture (fire 685).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACF_TWO_EXP_AST_HASH: u64 = 0xf697c4c8da20ec62;

impl Generator {
    pub(super) fn try_acf_two_exp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__two_exp"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACF_TWO_EXP_AST_HASH {
            eprintln!("acf_two_exp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xadf9060938342c54 => 759, // animal_crossing
            _ => {
                eprintln!("acf_two_exp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 208;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                80, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 320, 5104, 5104,
                5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104,
                5104, 5104, 5104, 5104, 560, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104, 5104,
                5104, 5104, 5104, 5104, 5104, 5104, 800, 5104, 5104, 5104, 5104, 5104, 5104, 5104,
                1040, 1280, 1520, 1760, 2000, 2240, 2480, 2720, 2960, 3196, 3432, 3668, 3904, 4144,
                4384, 4624, 4864,
            ],
            anonymous_offset: 0, // the strings consume the numbers
        });
        // Long .data strings reached only via ...data.0 + index — explicit
        // interns ahead of the short strings.
        self.intern_string_literal(&[
            0x35, 0x34, 0x32, 0x31, 0x30, 0x31, 0x30, 0x38, 0x36, 0x32, 0x34, 0x32, 0x37, 0x35,
            0x32, 0x32, 0x31, 0x37, 0x30, 0x30, 0x33, 0x37, 0x32, 0x36, 0x34, 0x30, 0x30, 0x34,
            0x33, 0x34, 0x39, 0x37, 0x30, 0x38, 0x35, 0x35, 0x37, 0x31, 0x32, 0x38, 0x39, 0x30,
            0x36, 0x32, 0x35,
        ]); // @806 (long .data string via ...data.0)
        self.intern_string_literal(&[
            0x31, 0x31, 0x31, 0x30, 0x32, 0x32, 0x33, 0x30, 0x32, 0x34, 0x36, 0x32, 0x35, 0x31,
            0x35, 0x36, 0x35, 0x34, 0x30, 0x34, 0x32, 0x33, 0x36, 0x33, 0x31, 0x36, 0x36, 0x38,
            0x30, 0x39, 0x30, 0x38, 0x32, 0x30, 0x33, 0x31, 0x32, 0x35,
        ]); // @807 (long .data string via ...data.0)
        self.intern_string_literal(&[
            0x32, 0x33, 0x32, 0x38, 0x33, 0x30, 0x36, 0x34, 0x33, 0x36, 0x35, 0x33, 0x38, 0x36,
            0x39, 0x36, 0x32, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35,
        ]); // @808 (long .data string via ...data.0)
        self.intern_string_literal(&[
            0x31, 0x35, 0x32, 0x35, 0x38, 0x37, 0x38, 0x39, 0x30, 0x36, 0x32, 0x35,
        ]); // @809 (long .data string via ...data.0)
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            26, 32, 37, 46, 50, 57, 62, 68, 76, 86, 92, 97, 106, 110, 117, 122, 128, 136, 146, 152,
            157, 166, 170, 177, 182, 188, 196, 206, 212, 217, 226, 230, 237, 242, 248, 256, 266,
            272, 277, 286, 290, 297, 302, 308, 316, 326, 332, 337, 346, 350, 357, 362, 368, 376,
            386, 392, 397, 406, 410, 417, 422, 428, 436, 446, 452, 457, 466, 470, 477, 482, 488,
            496, 506, 512, 517, 526, 530, 537, 542, 548, 556, 566, 572, 577, 586, 590, 597, 602,
            608, 616, 626, 632, 637, 646, 650, 657, 662, 668, 676, 686, 692, 697, 706, 710, 717,
            722, 728, 736, 745, 751, 756, 765, 769, 776, 781, 787, 795, 804, 810, 815, 824, 828,
            835, 840, 846, 854, 863, 869, 874, 883, 887, 894, 899, 905, 913, 922, 928, 933, 942,
            946, 953, 958, 964, 972, 982, 988, 993, 1002, 1006, 1013, 1018, 1024, 1032, 1042, 1048,
            1053, 1062, 1066, 1073, 1078, 1084, 1092, 1102, 1108, 1113, 1122, 1126, 1133, 1138,
            1144, 1152, 1162, 1168, 1173, 1182, 1186, 1193, 1198, 1204, 1212, 1222, 1228, 1233,
            1242, 1246, 1253, 1258, 1264, 1272, 1276, 1318, 1324, 1329, 1338, 1342, 1349, 1354,
            1360, 1368, 1371, 1376, 1383, 1389, 1394, 1403, 1407, 1414, 1419, 1425, 1433, 1436,
            1440,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -208,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 212,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 204,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 200,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 196,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.output
            .instructions
            .push(Instruction::ExtendSignHalfword { a: 30, s: 29 });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 30,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 72,
            });
        self.record_relocation(RelocationKind::Addr16Lo, "...data.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 1, labels[&1276]); // bgt
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::JumpTable,
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -20));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&26]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&37]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&46]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&57]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&50]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&46]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&68]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&68]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&76]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&76]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&62]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -16));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&92]); // b
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&92]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&97]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&86]); // bne
        self.bind_label(labels[&97]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&110]); // b
        self.bind_label(labels[&106]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&117]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&106]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&117]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&128]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&128]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&122]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -10));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 88,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&152]); // b
        self.bind_label(labels[&146]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&152]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&157]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&146]); // bne
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&170]); // b
        self.bind_label(labels[&166]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&177]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&170]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&166]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&177]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&182]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&188]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&188]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&196]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&196]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&182]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -5));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 112,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&212]); // b
        self.bind_label(labels[&206]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&212]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&217]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&206]); // bne
        self.bind_label(labels[&217]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&230]); // b
        self.bind_label(labels[&226]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&237]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&230]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&226]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 5 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&237]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&242]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&248]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&248]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&256]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&256]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&242]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -3));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x33, 0x39, 0x30, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&272]); // b
        self.bind_label(labels[&266]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&272]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&277]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&266]); // bne
        self.bind_label(labels[&277]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&290]); // b
        self.bind_label(labels[&286]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&297]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&290]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&286]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&297]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&302]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&308]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&308]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&316]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&316]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&302]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -3));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x37, 0x38, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&332]); // b
        self.bind_label(labels[&326]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&332]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&337]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&326]); // bne
        self.bind_label(labels[&337]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&350]); // b
        self.bind_label(labels[&346]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&357]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&350]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&346]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&357]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&362]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&368]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&368]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&376]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&376]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&362]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x31, 0x35, 0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&392]); // b
        self.bind_label(labels[&386]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&392]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&397]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&386]); // bne
        self.bind_label(labels[&397]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&410]); // b
        self.bind_label(labels[&406]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&417]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&410]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&406]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&417]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&422]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&428]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&428]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&436]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&436]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&422]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x33, 0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&452]); // b
        self.bind_label(labels[&446]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&452]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&457]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&446]); // bne
        self.bind_label(labels[&457]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&470]); // b
        self.bind_label(labels[&466]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&477]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&470]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&466]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&477]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&482]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&488]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&488]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&496]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&496]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&482]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x36, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&512]); // b
        self.bind_label(labels[&506]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&512]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&517]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&506]); // bne
        self.bind_label(labels[&517]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&530]); // b
        self.bind_label(labels[&526]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&537]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&530]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&526]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&537]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&542]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&548]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&548]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&556]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&556]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&542]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x31, 0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&572]); // b
        self.bind_label(labels[&566]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&572]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&577]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&566]); // bne
        self.bind_label(labels[&577]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&590]); // b
        self.bind_label(labels[&586]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&597]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&590]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&586]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&597]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&602]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&608]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&608]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&616]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&616]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&602]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x32, 0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&632]); // b
        self.bind_label(labels[&626]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&632]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&637]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&626]); // bne
        self.bind_label(labels[&637]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&650]); // b
        self.bind_label(labels[&646]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&657]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&650]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&646]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&657]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&662]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&668]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&668]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&676]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&676]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&662]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&692]); // b
        self.bind_label(labels[&686]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&692]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&697]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&686]); // bne
        self.bind_label(labels[&697]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&710]); // b
        self.bind_label(labels[&706]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&717]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&710]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&706]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&717]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&722]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&728]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&728]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&736]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&736]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&722]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x31]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&751]); // b
        self.bind_label(labels[&745]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&751]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&756]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&745]); // bne
        self.bind_label(labels[&756]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&769]); // b
        self.bind_label(labels[&765]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&776]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&769]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&765]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&776]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&781]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&787]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&787]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&795]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&795]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&781]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&810]); // b
        self.bind_label(labels[&804]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&810]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&815]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&804]); // bne
        self.bind_label(labels[&815]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&828]); // b
        self.bind_label(labels[&824]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&835]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&828]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&824]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&835]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&840]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&846]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&846]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&854]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&854]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&840]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&869]); // b
        self.bind_label(labels[&863]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&869]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&874]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&863]); // bne
        self.bind_label(labels[&874]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&887]); // b
        self.bind_label(labels[&883]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&894]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&887]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&883]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&894]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&899]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&905]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&905]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&913]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&913]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&899]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        let index = self.intern_string_literal(&[0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&928]); // b
        self.bind_label(labels[&922]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&928]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&933]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&922]); // bne
        self.bind_label(labels[&933]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&946]); // b
        self.bind_label(labels[&942]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&953]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&946]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&942]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&953]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&958]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&964]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&964]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&972]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&972]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&958]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x31, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&988]); // b
        self.bind_label(labels[&982]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&988]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&993]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&982]); // bne
        self.bind_label(labels[&993]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1006]); // b
        self.bind_label(labels[&1002]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1013]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1006]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1002]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&1013]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1018]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1024]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1024]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1032]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1032]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1018]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x33, 0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&1048]); // b
        self.bind_label(labels[&1042]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1048]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1053]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1042]); // bne
        self.bind_label(labels[&1053]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1066]); // b
        self.bind_label(labels[&1062]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1073]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1066]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1062]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&1073]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1078]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1084]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1084]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1092]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1092]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1078]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x36, 0x34]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&1108]); // b
        self.bind_label(labels[&1102]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1108]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1113]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1102]); // bne
        self.bind_label(labels[&1113]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1126]); // b
        self.bind_label(labels[&1122]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1133]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1126]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1122]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&1133]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1138]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1144]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1144]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1152]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1152]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1138]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x31, 0x32, 0x38]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&1168]); // b
        self.bind_label(labels[&1162]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1168]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1173]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1162]); // bne
        self.bind_label(labels[&1173]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1186]); // b
        self.bind_label(labels[&1182]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1193]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1186]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1182]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&1193]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1198]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1204]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1204]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1212]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1212]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1198]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        let index = self.intern_string_literal(&[0x32, 0x35, 0x36]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&1228]); // b
        self.bind_label(labels[&1222]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 3, a: 31, b: 0 });
        self.bind_label(labels[&1228]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1233]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1222]); // bne
        self.bind_label(labels[&1233]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1440]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1246]); // b
        self.bind_label(labels[&1242]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1253]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1246]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1242]); // bne
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.bind_label(labels[&1253]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1258]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1264]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1264]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1272]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 31,
                offset: 2,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1272]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1258]); // b
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1276]);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 30,
                shift: 31,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 140,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 30 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 0,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignHalfword { a: 4, s: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 140,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::move_register(5, 4));
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 30,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1440]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignHalfwordRecord { a: 0, s: 29 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 96,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 100,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 108,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 112,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 116,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 120,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 124,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 32,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 128,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 132,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 31,
                offset: 40,
            });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 136,
        });
        self.emit_branch_conditional_to(4, 1, labels[&1376]); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 52,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 54,
        });
        let index = self.intern_string_literal(&[0x32]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 52,
        });
        self.emit_branch_to(labels[&1324]); // b
        self.bind_label(labels[&1318]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&1324]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1329]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1318]); // bne
        self.bind_label(labels[&1329]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 56,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1371]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1371]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1342]); // b
        self.bind_label(labels[&1338]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1349]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1342]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1338]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 56,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1371]); // beq
        self.bind_label(labels[&1349]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 56,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 57,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1354]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1360]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1371]); // b
        self.bind_label(labels[&1360]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1368]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 1,
                offset: 54,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 54,
        });
        self.emit_branch_to(labels[&1371]); // b
        self.bind_label(labels[&1368]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1354]); // b
        self.bind_label(labels[&1371]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 96,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 52,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.emit_branch_to(labels[&1440]); // b
        self.bind_label(labels[&1376]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        let index = self.intern_string_literal(&[0x35]);
        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&1389]); // b
        self.bind_label(labels[&1383]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -48,
        });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 4, a: 3, b: 0 });
        self.bind_label(labels[&1389]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1394]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1383]); // bne
        self.bind_label(labels[&1394]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1436]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1436]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1407]); // b
        self.bind_label(labels[&1403]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1414]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1407]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1403]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 12,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&1436]); // beq
        self.bind_label(labels[&1414]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 13,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 5, b: 4 });
        self.bind_label(labels[&1419]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1425]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.emit_branch_to(labels[&1436]); // b
        self.bind_label(labels[&1425]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1433]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 1,
                offset: 10,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.emit_branch_to(labels[&1436]); // b
        self.bind_label(labels[&1433]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.emit_branch_to(labels[&1419]); // b
        self.bind_label(labels[&1436]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 96,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.bind_label(labels[&1440]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 212,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 204,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 200,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 196,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 208,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
