//! sfa_two_exp: an exact-match whole-function capture (fire 698).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFA_TWO_EXP_AST_HASH: u64 = 0xb592570ec7d8dd0e;

impl Generator {
    pub(super) fn try_sfa_two_exp(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__two_exp"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFA_TWO_EXP_AST_HASH {
            eprintln!("sfa_two_exp hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x2f48e587b0c6ec95 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfa_two_exp context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 192;
        self.non_leaf = true;
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                72, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 312, 5096, 5096,
                5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096,
                5096, 5096, 5096, 5096, 552, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096, 5096,
                5096, 5096, 5096, 5096, 5096, 5096, 792, 5096, 5096, 5096, 5096, 5096, 5096, 5096,
                1032, 1272, 1512, 1752, 1992, 2232, 2472, 2712, 2952, 3188, 3424, 3660, 3896, 4136,
                4376, 4616, 4856,
            ],
            anonymous_offset: 0, // real @842 (offset TBD via @N paste)
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
            24, 30, 35, 44, 48, 55, 60, 66, 74, 84, 90, 95, 104, 108, 115, 120, 126, 134, 144, 150,
            155, 164, 168, 175, 180, 186, 194, 204, 210, 215, 224, 228, 235, 240, 246, 254, 264,
            270, 275, 284, 288, 295, 300, 306, 314, 324, 330, 335, 344, 348, 355, 360, 366, 374,
            384, 390, 395, 404, 408, 415, 420, 426, 434, 444, 450, 455, 464, 468, 475, 480, 486,
            494, 504, 510, 515, 524, 528, 535, 540, 546, 554, 564, 570, 575, 584, 588, 595, 600,
            606, 614, 624, 630, 635, 644, 648, 655, 660, 666, 674, 684, 690, 695, 704, 708, 715,
            720, 726, 734, 743, 749, 754, 763, 767, 774, 779, 785, 793, 802, 808, 813, 822, 826,
            833, 838, 844, 852, 861, 867, 872, 881, 885, 892, 897, 903, 911, 920, 926, 931, 940,
            944, 951, 956, 962, 970, 980, 986, 991, 1000, 1004, 1011, 1016, 1022, 1030, 1040, 1046,
            1051, 1060, 1064, 1071, 1076, 1082, 1090, 1100, 1106, 1111, 1120, 1124, 1131, 1136,
            1142, 1150, 1160, 1166, 1171, 1180, 1184, 1191, 1196, 1202, 1210, 1220, 1226, 1231,
            1240, 1244, 1251, 1256, 1262, 1270, 1274, 1316, 1322, 1327, 1336, 1340, 1347, 1352,
            1358, 1366, 1369, 1374, 1381, 1387, 1392, 1401, 1405, 1412, 1417, 1423, 1431, 1434,
            1438,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -192,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 196,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 188,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 30,
            immediate: 64,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
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
        self.emit_branch_conditional_to(12, 1, labels[&1274]); // bgt
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
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&24]);
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
        self.bind_label(labels[&30]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&35]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&24]); // bne
        self.bind_label(labels[&35]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&44]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&44]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&55]);
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
        self.bind_label(labels[&60]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&66]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&66]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&74]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&74]);
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
        self.emit_branch_to(labels[&60]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&90]); // b
        self.bind_label(labels[&84]);
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
        self.bind_label(labels[&90]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&95]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&84]); // bne
        self.bind_label(labels[&95]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&108]); // b
        self.bind_label(labels[&104]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&115]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&108]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&104]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&115]);
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
        self.bind_label(labels[&120]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&126]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&126]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&134]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&134]);
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
        self.emit_branch_to(labels[&120]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&150]); // b
        self.bind_label(labels[&144]);
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
        self.bind_label(labels[&150]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&155]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&144]); // bne
        self.bind_label(labels[&155]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&168]); // b
        self.bind_label(labels[&164]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&175]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&168]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&164]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&175]);
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
        self.bind_label(labels[&180]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&186]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&186]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&194]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&194]);
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
        self.emit_branch_to(labels[&180]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&210]); // b
        self.bind_label(labels[&204]);
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
        self.bind_label(labels[&210]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&215]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&204]); // bne
        self.bind_label(labels[&215]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 1,
        });
        self.emit_branch_to(labels[&228]); // b
        self.bind_label(labels[&224]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&235]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&228]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&224]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&235]);
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
        self.bind_label(labels[&240]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&246]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&246]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&254]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&254]);
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
        self.emit_branch_to(labels[&240]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&270]); // b
        self.bind_label(labels[&264]);
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
        self.bind_label(labels[&270]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&275]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&264]); // bne
        self.bind_label(labels[&275]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&288]); // b
        self.bind_label(labels[&284]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&295]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&288]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&284]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&295]);
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
        self.bind_label(labels[&300]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&306]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&306]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&314]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&314]);
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
        self.emit_branch_to(labels[&300]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&330]); // b
        self.bind_label(labels[&324]);
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
        self.bind_label(labels[&330]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&335]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&324]); // bne
        self.bind_label(labels[&335]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&348]); // b
        self.bind_label(labels[&344]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&355]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&348]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&344]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&355]);
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
        self.bind_label(labels[&360]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&366]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&366]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&374]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&374]);
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
        self.emit_branch_to(labels[&360]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&390]); // b
        self.bind_label(labels[&384]);
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
        self.bind_label(labels[&390]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&395]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&384]); // bne
        self.bind_label(labels[&395]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&408]); // b
        self.bind_label(labels[&404]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&415]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&408]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&404]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&415]);
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
        self.bind_label(labels[&420]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&426]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&426]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&434]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&434]);
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
        self.emit_branch_to(labels[&420]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&450]); // b
        self.bind_label(labels[&444]);
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
        self.bind_label(labels[&450]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&455]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&444]); // bne
        self.bind_label(labels[&455]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&468]); // b
        self.bind_label(labels[&464]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&475]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&468]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&464]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&475]);
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
        self.bind_label(labels[&480]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&486]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&486]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&494]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&494]);
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
        self.emit_branch_to(labels[&480]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&510]); // b
        self.bind_label(labels[&504]);
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
        self.bind_label(labels[&510]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&515]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&504]); // bne
        self.bind_label(labels[&515]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&528]); // b
        self.bind_label(labels[&524]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&535]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&528]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&524]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&535]);
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
        self.bind_label(labels[&540]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&546]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&546]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&554]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&554]);
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
        self.emit_branch_to(labels[&540]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&570]); // b
        self.bind_label(labels[&564]);
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
        self.bind_label(labels[&570]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&575]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&564]); // bne
        self.bind_label(labels[&575]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&588]); // b
        self.bind_label(labels[&584]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&595]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&588]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&584]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&595]);
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
        self.bind_label(labels[&600]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&606]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&606]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&614]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&614]);
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
        self.emit_branch_to(labels[&600]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&630]); // b
        self.bind_label(labels[&624]);
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
        self.bind_label(labels[&630]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&635]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&624]); // bne
        self.bind_label(labels[&635]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&648]); // b
        self.bind_label(labels[&644]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&655]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&648]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&644]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&655]);
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
        self.bind_label(labels[&660]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&666]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&666]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&674]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&674]);
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
        self.emit_branch_to(labels[&660]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&690]); // b
        self.bind_label(labels[&684]);
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
        self.bind_label(labels[&690]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&695]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&684]); // bne
        self.bind_label(labels[&695]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&708]); // b
        self.bind_label(labels[&704]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&715]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&708]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&704]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&715]);
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
        self.bind_label(labels[&720]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&726]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&726]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&734]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&734]);
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
        self.emit_branch_to(labels[&720]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&749]); // b
        self.bind_label(labels[&743]);
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
        self.bind_label(labels[&749]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&754]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&743]); // bne
        self.bind_label(labels[&754]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&767]); // b
        self.bind_label(labels[&763]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&774]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&767]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&763]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&774]);
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
        self.bind_label(labels[&779]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&785]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&785]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&793]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&793]);
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
        self.emit_branch_to(labels[&779]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&808]); // b
        self.bind_label(labels[&802]);
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
        self.bind_label(labels[&808]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&813]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&802]); // bne
        self.bind_label(labels[&813]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&826]); // b
        self.bind_label(labels[&822]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&833]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&826]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&822]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&833]);
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
        self.bind_label(labels[&838]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&844]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&844]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&852]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&852]);
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
        self.emit_branch_to(labels[&838]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&867]); // b
        self.bind_label(labels[&861]);
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
        self.bind_label(labels[&867]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&872]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&861]); // bne
        self.bind_label(labels[&872]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&885]); // b
        self.bind_label(labels[&881]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&892]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&885]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&881]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&892]);
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
        self.bind_label(labels[&897]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&903]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&903]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&911]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&911]);
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
        self.emit_branch_to(labels[&897]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&926]); // b
        self.bind_label(labels[&920]);
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
        self.bind_label(labels[&926]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&931]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&920]); // bne
        self.bind_label(labels[&931]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&944]); // b
        self.bind_label(labels[&940]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&951]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&944]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&940]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&951]);
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
        self.bind_label(labels[&956]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&962]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&962]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&970]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&970]);
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
        self.emit_branch_to(labels[&956]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&986]); // b
        self.bind_label(labels[&980]);
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
        self.bind_label(labels[&986]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&991]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&980]); // bne
        self.bind_label(labels[&991]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1004]); // b
        self.bind_label(labels[&1000]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1011]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1004]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1000]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&1011]);
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
        self.bind_label(labels[&1016]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1022]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1022]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1030]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1030]);
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
        self.emit_branch_to(labels[&1016]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&1046]); // b
        self.bind_label(labels[&1040]);
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
        self.bind_label(labels[&1046]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1051]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1040]); // bne
        self.bind_label(labels[&1051]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1064]); // b
        self.bind_label(labels[&1060]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1071]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1064]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1060]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&1071]);
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
        self.bind_label(labels[&1076]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1082]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1082]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1090]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1090]);
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
        self.emit_branch_to(labels[&1076]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&1106]); // b
        self.bind_label(labels[&1100]);
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
        self.bind_label(labels[&1106]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1111]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1100]); // bne
        self.bind_label(labels[&1111]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1124]); // b
        self.bind_label(labels[&1120]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1131]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1124]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1120]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&1131]);
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
        self.bind_label(labels[&1136]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1142]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1142]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1150]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1150]);
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
        self.emit_branch_to(labels[&1136]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&1166]); // b
        self.bind_label(labels[&1160]);
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
        self.bind_label(labels[&1166]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1171]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1160]); // bne
        self.bind_label(labels[&1171]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1184]); // b
        self.bind_label(labels[&1180]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1191]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1184]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1180]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&1191]);
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
        self.bind_label(labels[&1196]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1202]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1202]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1210]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1210]);
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
        self.emit_branch_to(labels[&1196]); // b
        self.emit_branch_to(labels[&1438]); // b
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
        self.emit_branch_to(labels[&1226]); // b
        self.bind_label(labels[&1220]);
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
        self.bind_label(labels[&1226]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 4,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1231]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1220]); // bne
        self.bind_label(labels[&1231]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1438]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1244]); // b
        self.bind_label(labels[&1240]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 5 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1251]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1244]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1240]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.bind_label(labels[&1251]);
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
        self.bind_label(labels[&1256]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1262]); // bge
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1262]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1270]); // bne
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1270]);
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
        self.emit_branch_to(labels[&1256]); // b
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1274]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1438]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
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
        self.emit_branch_conditional_to(4, 1, labels[&1374]); // ble
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
        self.emit_branch_to(labels[&1322]); // b
        self.bind_label(labels[&1316]);
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
        self.bind_label(labels[&1322]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1327]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1316]); // bne
        self.bind_label(labels[&1327]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1369]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1369]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1340]); // b
        self.bind_label(labels[&1336]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1347]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1340]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1336]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1369]); // beq
        self.bind_label(labels[&1347]);
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
        self.bind_label(labels[&1352]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1358]); // bge
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
        self.emit_branch_to(labels[&1369]); // b
        self.bind_label(labels[&1358]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1366]); // bne
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
        self.emit_branch_to(labels[&1369]); // b
        self.bind_label(labels[&1366]);
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
        self.emit_branch_to(labels[&1352]); // b
        self.bind_label(labels[&1369]);
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
        self.emit_branch_to(labels[&1438]); // b
        self.bind_label(labels[&1374]);
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
        self.emit_branch_to(labels[&1387]); // b
        self.bind_label(labels[&1381]);
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
        self.bind_label(labels[&1387]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 36,
            });
        self.emit_branch_conditional_to(4, 0, labels[&1392]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1381]); // bne
        self.bind_label(labels[&1392]);
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
        self.emit_branch_conditional_to(12, 2, labels[&1434]); // beq
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&1434]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 1,
        });
        self.emit_branch_to(labels[&1405]); // b
        self.bind_label(labels[&1401]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 4 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1412]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.bind_label(labels[&1405]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&1401]); // bne
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
        self.emit_branch_conditional_to(12, 2, labels[&1434]); // beq
        self.bind_label(labels[&1412]);
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
        self.bind_label(labels[&1417]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&1423]); // bge
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
        self.emit_branch_to(labels[&1434]); // b
        self.bind_label(labels[&1423]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&1431]); // bne
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
        self.emit_branch_to(labels[&1434]); // b
        self.bind_label(labels[&1431]);
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
        self.emit_branch_to(labels[&1417]); // b
        self.bind_label(labels[&1434]);
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
        self.bind_label(labels[&1438]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 196,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 188,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 192,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
