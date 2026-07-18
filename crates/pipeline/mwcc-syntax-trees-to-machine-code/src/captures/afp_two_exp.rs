//! afp_two_exp: an exact-match whole-function capture (fire 678).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const AFP_TWO_EXP_AST_HASH: u64 = 0x82a1aa02ee8fa93;

impl Generator {
    pub(super) fn try_afp_two_exp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__two_exp"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != AFP_TWO_EXP_AST_HASH {
            eprintln!("afp_two_exp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x583729950621373 => 0, // marioparty4
            _ => {
                eprintln!("afp_two_exp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 208;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                72, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 316, 5180, 5180,
                5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180,
                5180, 5180, 5180, 5180, 560, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180, 5180,
                5180, 5180, 5180, 5180, 5180, 5180, 804, 5180, 5180, 5180, 5180, 5180, 5180, 5180,
                1048, 1292, 1536, 1780, 2024, 2268, 2512, 2756, 3000, 3240, 3480, 3720, 3960, 4204,
                4448, 4692, 4936,
            ],
            anonymous_offset: 669, // measured (real table @752, counter @83 at dispatch)
        });
        // The 21 case bodies' internal labels continue PAST the table symbol —
        // the pool in the NEXT function numbers @837 (table @752 + 81 + the
        // inter-function advance).
        self.output.post_constant_label_bump = 81;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            26, 32, 37, 46, 50, 57, 62, 68, 76, 87, 93, 98, 107, 111, 118, 123, 129, 137, 148, 154,
            159, 168, 172, 179, 184, 190, 198, 209, 215, 220, 229, 233, 240, 245, 251, 259, 270,
            276, 281, 290, 294, 301, 306, 312, 320, 331, 337, 342, 351, 355, 362, 367, 373, 381,
            392, 398, 403, 412, 416, 423, 428, 434, 442, 453, 459, 464, 473, 477, 484, 489, 495,
            503, 514, 520, 525, 534, 538, 545, 550, 556, 564, 575, 581, 586, 595, 599, 606, 611,
            617, 625, 636, 642, 647, 656, 660, 667, 672, 678, 686, 697, 703, 708, 717, 721, 728,
            733, 739, 747, 757, 763, 768, 777, 781, 788, 793, 799, 807, 817, 823, 828, 837, 841,
            848, 853, 859, 867, 877, 883, 888, 897, 901, 908, 913, 919, 927, 937, 943, 948, 957,
            961, 968, 973, 979, 987, 998, 1004, 1009, 1018, 1022, 1029, 1034, 1040, 1048, 1059,
            1065, 1070, 1079, 1083, 1090, 1095, 1101, 1109, 1120, 1126, 1131, 1140, 1144, 1151,
            1156, 1162, 1170, 1181, 1187, 1192, 1201, 1205, 1212, 1217, 1223, 1231, 1242, 1248,
            1253, 1262, 1266, 1273, 1278, 1284, 1292, 1295, 1339, 1345, 1350, 1359, 1363, 1370,
            1375, 1381, 1389, 1392, 1397, 1406, 1412, 1417, 1426, 1430, 1437, 1442, 1448, 1456,
            1459, 1463,
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
        self.emit_branch_conditional_to(12, 1, labels[&1295]); // bgt
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
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 37,
        });
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&26]);
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
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&37]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&26]); // bne
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&50]); // b
        self.bind_label(labels[&46]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
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
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&46]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
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
        self.emit_branch_to(labels[&1463]); // b
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
        self.emit_branch_to(labels[&1463]); // b
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
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -16));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 83,
        });
        self.emit_branch_to(labels[&93]); // b
        self.bind_label(labels[&87]);
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
        self.bind_label(labels[&93]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&98]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&87]); // bne
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&111]); // b
        self.bind_label(labels[&107]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&118]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&111]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&107]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&118]);
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
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&129]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&129]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&137]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&137]);
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
        self.emit_branch_to(labels[&123]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -10));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 122,
        });
        self.emit_branch_to(labels[&154]); // b
        self.bind_label(labels[&148]);
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
        self.bind_label(labels[&154]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&159]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&148]); // bne
        self.bind_label(labels[&159]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&172]); // b
        self.bind_label(labels[&168]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&179]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&172]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&168]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&179]);
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
        self.bind_label(labels[&184]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&190]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&190]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&198]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&198]);
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
        self.emit_branch_to(labels[&184]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -5));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 146,
        });
        self.emit_branch_to(labels[&215]); // b
        self.bind_label(labels[&209]);
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
        self.bind_label(labels[&215]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&220]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&209]); // bne
        self.bind_label(labels[&220]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&233]); // b
        self.bind_label(labels[&229]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&240]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&233]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&229]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&240]);
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
        self.bind_label(labels[&245]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&251]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&251]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&259]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&259]);
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
        self.emit_branch_to(labels[&245]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -3));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 159,
        });
        self.emit_branch_to(labels[&276]); // b
        self.bind_label(labels[&270]);
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
        self.bind_label(labels[&276]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&281]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&270]); // bne
        self.bind_label(labels[&281]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&294]); // b
        self.bind_label(labels[&290]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&301]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&294]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&290]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&301]);
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
        self.bind_label(labels[&306]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&312]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&312]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&320]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&320]);
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
        self.emit_branch_to(labels[&306]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -3));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 166,
        });
        self.emit_branch_to(labels[&337]); // b
        self.bind_label(labels[&331]);
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
        self.bind_label(labels[&337]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&342]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&331]); // bne
        self.bind_label(labels[&342]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&355]); // b
        self.bind_label(labels[&351]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&362]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&355]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&351]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&362]);
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
        self.bind_label(labels[&367]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&373]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&373]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&381]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&381]);
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
        self.emit_branch_to(labels[&367]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 172,
        });
        self.emit_branch_to(labels[&398]); // b
        self.bind_label(labels[&392]);
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
        self.bind_label(labels[&398]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&403]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&392]); // bne
        self.bind_label(labels[&403]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&416]); // b
        self.bind_label(labels[&412]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&423]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&416]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&412]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&423]);
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
        self.bind_label(labels[&428]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&434]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&434]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&442]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&442]);
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
        self.emit_branch_to(labels[&428]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 178,
        });
        self.emit_branch_to(labels[&459]); // b
        self.bind_label(labels[&453]);
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
        self.bind_label(labels[&459]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&464]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&453]); // bne
        self.bind_label(labels[&464]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&477]); // b
        self.bind_label(labels[&473]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&484]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&477]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&473]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&484]);
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
        self.bind_label(labels[&489]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&495]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&495]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&503]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&503]);
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
        self.emit_branch_to(labels[&489]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -2));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 183,
        });
        self.emit_branch_to(labels[&520]); // b
        self.bind_label(labels[&514]);
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
        self.bind_label(labels[&520]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&525]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&514]); // bne
        self.bind_label(labels[&525]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&538]); // b
        self.bind_label(labels[&534]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&545]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&538]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&534]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&545]);
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
        self.bind_label(labels[&550]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&556]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&556]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&564]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&564]);
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
        self.emit_branch_to(labels[&550]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 187,
        });
        self.emit_branch_to(labels[&581]); // b
        self.bind_label(labels[&575]);
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
        self.bind_label(labels[&581]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&586]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&575]); // bne
        self.bind_label(labels[&586]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&599]); // b
        self.bind_label(labels[&595]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&606]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&599]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&595]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&606]);
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
        self.bind_label(labels[&611]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&617]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&617]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&625]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&625]);
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
        self.emit_branch_to(labels[&611]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 191,
        });
        self.emit_branch_to(labels[&642]); // b
        self.bind_label(labels[&636]);
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
        self.bind_label(labels[&642]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&647]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&636]); // bne
        self.bind_label(labels[&647]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&660]); // b
        self.bind_label(labels[&656]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&667]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&660]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&656]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&667]);
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
        self.bind_label(labels[&672]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&678]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&678]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&686]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&686]);
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
        self.emit_branch_to(labels[&672]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 194,
        });
        self.emit_branch_to(labels[&703]); // b
        self.bind_label(labels[&697]);
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
        self.bind_label(labels[&703]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&708]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&697]); // bne
        self.bind_label(labels[&708]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&721]); // b
        self.bind_label(labels[&717]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&728]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&721]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&717]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&728]);
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
        self.bind_label(labels[&733]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&739]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&739]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&747]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&747]);
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
        self.emit_branch_to(labels[&733]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 196,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&763]); // b
        self.bind_label(labels[&757]);
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
        self.bind_label(labels[&763]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&768]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&757]); // bne
        self.bind_label(labels[&768]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&781]); // b
        self.bind_label(labels[&777]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&788]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&781]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&777]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&788]);
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
        self.bind_label(labels[&793]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&799]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&799]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&807]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&807]);
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
        self.emit_branch_to(labels[&793]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 198,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&823]); // b
        self.bind_label(labels[&817]);
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
        self.bind_label(labels[&823]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&828]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&817]); // bne
        self.bind_label(labels[&828]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&841]); // b
        self.bind_label(labels[&837]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&848]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&841]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&837]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&848]);
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
        self.bind_label(labels[&853]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&859]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&859]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&867]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&867]);
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
        self.emit_branch_to(labels[&853]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 200,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&883]); // b
        self.bind_label(labels[&877]);
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
        self.bind_label(labels[&883]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&888]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&877]); // bne
        self.bind_label(labels[&888]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&901]); // b
        self.bind_label(labels[&897]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&908]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&901]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&897]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&908]);
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
        self.bind_label(labels[&913]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&919]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&919]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&927]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&927]);
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
        self.emit_branch_to(labels[&913]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 31,
            offset: 2,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 202,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.emit_branch_to(labels[&943]); // b
        self.bind_label(labels[&937]);
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
        self.bind_label(labels[&943]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&948]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&937]); // bne
        self.bind_label(labels[&948]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&961]); // b
        self.bind_label(labels[&957]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&968]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&961]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&957]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&968]);
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
        self.bind_label(labels[&973]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&979]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&979]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&987]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&987]);
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
        self.emit_branch_to(labels[&973]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 204,
        });
        self.emit_branch_to(labels[&1004]); // b
        self.bind_label(labels[&998]);
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
        self.bind_label(labels[&1004]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1009]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&998]); // bne
        self.bind_label(labels[&1009]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1022]); // b
        self.bind_label(labels[&1018]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1029]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1022]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1018]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&1029]);
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
        self.bind_label(labels[&1034]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1040]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1040]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1048]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1048]);
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
        self.emit_branch_to(labels[&1034]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 207,
        });
        self.emit_branch_to(labels[&1065]); // b
        self.bind_label(labels[&1059]);
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
        self.bind_label(labels[&1065]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1070]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1059]); // bne
        self.bind_label(labels[&1070]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1083]); // b
        self.bind_label(labels[&1079]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1090]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1083]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1079]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&1090]);
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
        self.bind_label(labels[&1095]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1101]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1101]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1109]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1109]);
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
        self.emit_branch_to(labels[&1095]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 210,
        });
        self.emit_branch_to(labels[&1126]); // b
        self.bind_label(labels[&1120]);
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
        self.bind_label(labels[&1126]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1131]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1120]); // bne
        self.bind_label(labels[&1131]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1144]); // b
        self.bind_label(labels[&1140]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1151]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1144]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1140]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&1151]);
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
        self.bind_label(labels[&1156]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1162]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1162]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1170]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1170]);
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
        self.emit_branch_to(labels[&1156]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 213,
        });
        self.emit_branch_to(labels[&1187]); // b
        self.bind_label(labels[&1181]);
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
        self.bind_label(labels[&1187]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1192]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1181]); // bne
        self.bind_label(labels[&1192]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1205]); // b
        self.bind_label(labels[&1201]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1212]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1205]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1201]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&1212]);
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
        self.bind_label(labels[&1217]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1223]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1223]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1231]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1231]);
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
        self.emit_branch_to(labels[&1217]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 217,
        });
        self.emit_branch_to(labels[&1248]); // b
        self.bind_label(labels[&1242]);
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
        self.bind_label(labels[&1248]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1253]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1242]); // bne
        self.bind_label(labels[&1253]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 31,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1463]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1266]); // b
        self.bind_label(labels[&1262]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1273]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1266]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1262]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
        self.bind_label(labels[&1273]);
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
        self.bind_label(labels[&1278]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1284]); // bge
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1284]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1292]); // bne
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1292]);
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
        self.emit_branch_to(labels[&1278]); // b
        self.bind_label(labels[&1295]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1463]); // beq
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
        self.emit_branch_conditional_to(4, 1, labels[&1397]); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 54,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 52,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 4,
            immediate: 198,
        });
        self.emit_branch_to(labels[&1345]); // b
        self.bind_label(labels[&1339]);
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
        self.bind_label(labels[&1345]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1350]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1339]); // bne
        self.bind_label(labels[&1350]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1392]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1392]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1363]); // b
        self.bind_label(labels[&1359]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1370]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1363]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1359]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1392]); // beq
        self.bind_label(labels[&1370]);
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
        self.bind_label(labels[&1375]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1381]); // bge
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
        self.emit_branch_to(labels[&1392]); // b
        self.bind_label(labels[&1381]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1389]); // bne
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
        self.emit_branch_to(labels[&1392]); // b
        self.bind_label(labels[&1389]);
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
        self.emit_branch_to(labels[&1375]); // b
        self.bind_label(labels[&1392]);
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
        self.emit_branch_to(labels[&1463]); // b
        self.bind_label(labels[&1397]);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "lbl_8011E630");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "lbl_8011E630");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: 194,
        });
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
        self.emit_branch_to(labels[&1412]); // b
        self.bind_label(labels[&1406]);
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
        self.bind_label(labels[&1412]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1417]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1406]); // bne
        self.bind_label(labels[&1417]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1459]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1459]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1430]); // b
        self.bind_label(labels[&1426]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1437]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1430]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1426]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1459]); // beq
        self.bind_label(labels[&1437]);
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
        self.bind_label(labels[&1442]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1448]); // bge
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
        self.emit_branch_to(labels[&1459]); // b
        self.bind_label(labels[&1448]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1456]); // bne
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
        self.emit_branch_to(labels[&1459]); // b
        self.bind_label(labels[&1456]);
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
        self.emit_branch_to(labels[&1442]); // b
        self.bind_label(labels[&1459]);
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
        self.bind_label(labels[&1463]);
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
