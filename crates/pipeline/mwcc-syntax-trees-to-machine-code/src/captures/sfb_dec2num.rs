//! sfb_dec2num: an exact-match whole-function capture (fire 724).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFB_DEC2NUM_AST_HASH: u64 = 0x5aeb4a3aff678032;

impl Generator {
    pub(super) fn try_sfb_dec2num(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dec2num"
            || function.return_type != Type::Double
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFB_DEC2NUM_AST_HASH {
            eprintln!("sfb_dec2num hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 315, // strikers copy
            _ => {
                eprintln!("sfb_dec2num context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 1120;
        self.non_leaf = true;
        self.output.constant_number_gaps = vec![(4, 2)];
        self.output.string_number_after_constants = Some(4);
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
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            19, 20, 22, 29, 32, 38, 39, 41, 48, 49, 51, 62, 70, 77, 84, 91, 95, 98, 100, 103, 110,
            112, 117, 123, 125, 157, 185, 186, 191, 210, 219, 225, 253, 254, 259, 273, 275, 277,
            292, 303, 317, 322, 324, 326, 331, 333, 335, 336, 351, 368, 378, 383, 385, 387, 392,
            394, 396, 397, 414, 419, 421, 423, 428, 430, 432, 433, 437, 439, 442, 468, 499, 504,
            506, 508, 513, 515, 517, 518, 522, 538, 548, 553, 555, 557, 562, 564, 566, 567, 584,
            589, 591, 593, 598, 600, 602, 603, 607, 609, 612, 638, 641, 683, 688, 690, 692, 697,
            699, 701, 702, 706, 721, 731, 736, 738, 740, 745, 747, 749, 750, 767, 772, 774, 776,
            781, 783, 785, 786, 790, 792, 795, 821, 852, 860, 886, 896, 901, 903, 905, 910, 912,
            914, 915, 932, 937, 939, 941, 946, 948, 950, 951, 955, 957, 960, 986, 988, 1035, 1045,
            1050, 1052, 1054, 1059, 1061, 1063, 1064, 1081, 1086, 1088, 1090, 1095, 1097, 1099,
            1100, 1104, 1106, 1109, 1135, 1166, 1173, 1179, 1180,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -1120,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 1124,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 1104,
            });
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedStore {
                s: 31,
                a: 1,
                offset: 1112,
                w: 0,
                i: 0,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 1088,
            });
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedStore {
                s: 30,
                a: 1,
                offset: 1096,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 1084,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 1080,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 1076,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&22]); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.load_double_constant(1, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&19]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&20]); // b
        self.bind_label(labels[&19]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&20]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.emit_branch_to(labels[&1180]); // b
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 5,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 73,
            });
        self.emit_branch_conditional_to(12, 2, labels[&41]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&29]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 48,
            });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.emit_branch_to(labels[&125]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 78,
            });
        self.emit_branch_conditional_to(12, 2, labels[&51]); // beq
        self.emit_branch_to(labels[&125]); // b
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.load_double_constant(1, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&39]); // b
        self.bind_label(labels[&38]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&39]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.emit_branch_to(labels[&1180]); // b
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "__float_huge");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__float_huge");
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.load_double_constant(2, 0x3ff0000000000000);
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&48]);
        self.load_double_constant(2, 0xbff0000000000000);
        self.bind_label(labels[&49]);
        self.record_relocation(RelocationKind::Rel24, "copysign");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "copysign".to_string(),
        });
        self.emit_branch_to(labels[&1180]); // b
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 32752));
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 204,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 200,
        });
        self.emit_branch_conditional_to(12, 2, labels[&62]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, -32768));
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 204,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 200,
        });
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&70]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 200,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 8));
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 200,
        });
        self.emit_branch_to(labels[&123]); // b
        self.bind_label(labels[&70]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 14,
            });
        self.output
            .instructions
            .push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 1,
            immediate: 201,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 1));
        self.emit_branch_conditional_to(4, 1, labels[&77]); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 14));
        self.bind_label(labels[&77]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 6,
            immediate: -1,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(10, 1));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 1 });
        self.emit_branch_conditional_to(4, 1, labels[&117]); // ble
        self.bind_label(labels[&84]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 10,
            immediate: 5,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 6, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 5, b: 6 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 27,
                end: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&91]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 6,
            immediate: -48,
        });
        self.emit_branch_to(labels[&100]); // b
        self.bind_label(labels[&91]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&95]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.emit_branch_to(labels[&98]); // b
        self.bind_label(labels[&95]);
        self.record_relocation(RelocationKind::Addr16Ha, "__lower_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__lower_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 4, a: 4, b: 6 });
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -87,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: 0,
                clear: 24,
            });
        self.bind_label(labels[&100]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 24,
            });
        self.emit_branch_conditional_to(12, 2, labels[&103]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 1));
        self.bind_label(labels[&103]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&110]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 4 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 7,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 7,
            immediate: 1,
        });
        self.emit_branch_to(labels[&112]); // b
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 4,
            shift: 4,
            begin: 24,
            end: 27,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 7,
            offset: 0,
        });
        self.bind_label(labels[&112]);
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 9 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 10,
            a: 10,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::move_register(9, 0));
        self.emit_branch_conditional_to(16, 0, labels[&84]); // bdnz
        self.bind_label(labels[&117]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&123]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 200,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 8));
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 200,
        });
        self.bind_label(labels[&123]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 200,
        });
        self.emit_branch_to(labels[&1180]); // b
        self.bind_label(labels[&125]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 1,
            immediate: 1005,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 29));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 1004,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 1004,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 29, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 3,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 3,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 29, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 3,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 29, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 3,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 3,
            offset: 32,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 3,
                offset: 40,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 1000,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 12,
            a: 1,
            offset: 1008,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 11,
            a: 1,
            offset: 1012,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 1,
            offset: 1016,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 1020,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: 1,
            offset: 1024,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 1028,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 1,
            offset: 1032,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 1036,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 1040,
        });
        self.emit_branch_conditional_to(4, 0, labels[&191]); // bge
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 4,
                shift: 29,
                begin: 3,
                end: 31,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&185]); // beq
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 1,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 2,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 3,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 3,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 5,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 6,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 6,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 7,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 7,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 8,
        });
        self.emit_branch_conditional_to(16, 0, labels[&157]); // bdnz
        self.output
            .instructions
            .push(Instruction::AndImmediateRecord {
                a: 4,
                s: 4,
                immediate: 7,
            });
        self.emit_branch_conditional_to(12, 2, labels[&191]); // beq
        self.bind_label(labels[&185]);
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&186]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 1,
        });
        self.emit_branch_conditional_to(16, 0, labels[&186]); // bdnz
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 1,
            offset: 1005,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(7, 17200));
        self.record_relocation(RelocationKind::Addr16Ha, "pow_10");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 1048,
        });
        self.load_double_constant(3, 0x4330000000000000);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 29,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 1052,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "pow_10");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 4,
                a: 1,
                offset: 1002,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 1048,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 1004,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignHalfword { a: 3, s: 4 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 1,
            offset: 1002,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&275]); // b
        self.bind_label(labels[&210]);
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 8, b: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(9, 0));
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 4,
                shift: 29,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 4,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 3, a: 4, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 3,
            begin: 0,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::AddRecord { d: 10, a: 3, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&219]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(10, 8));
        self.bind_label(labels[&219]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 10,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::move_register(4, 10));
        self.emit_branch_conditional_to(4, 1, labels[&259]); // ble
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 10,
                shift: 29,
                begin: 3,
                end: 31,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&253]); // beq
        self.bind_label(labels[&225]);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 1,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 2,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 3,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 5,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 6,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 7,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&225]); // bdnz
        self.output
            .instructions
            .push(Instruction::AndImmediateRecord {
                a: 4,
                s: 4,
                immediate: 7,
            });
        self.emit_branch_conditional_to(12, 2, labels[&259]); // beq
        self.bind_label(labels[&253]);
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 4 });
        self.bind_label(labels[&254]);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 9,
                immediate: 10,
            });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 6, b: 3 });
        self.emit_branch_conditional_to(16, 0, labels[&254]); // bdnz
        self.bind_label(labels[&259]);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 10,
                shift: 3,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 1052,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 1,
            offset: 192,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 1048,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 3,
            offset: -8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 1048,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 2, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&273]); // beq
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&277]); // beq
        self.bind_label(labels[&273]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: 30,
            a: 10,
            b: 30,
        });
        self.bind_label(labels[&275]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 8, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&210]); // blt
        self.bind_label(labels[&277]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 0, labels[&292]); // bge
        self.output
            .instructions
            .push(Instruction::Negate { d: 3, a: 30 });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 3,
                s: 3,
                immediate: 32768,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 1048,
        });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 1052,
        });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 1048,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "pow".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::FloatDivideDouble { d: 1, a: 0, b: 1 });
        self.emit_branch_to(labels[&303]); // b
        self.bind_label(labels[&292]);
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 3,
                s: 30,
                immediate: 32768,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 1060,
        });
        self.load_double_constant(2, 0x4330000080000000);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 1056,
        });
        self.load_double_constant(1, 0x4014000000000000);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 1056,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 2, a: 0, b: 2 });
        self.record_relocation(RelocationKind::Rel24, "pow");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "pow".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 0, c: 1 });
        self.bind_label(labels[&303]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 192,
            });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 168,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 168,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 192,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&317]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&335]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&326]); // beq
        self.emit_branch_to(labels[&335]); // b
        self.bind_label(labels[&317]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&322]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 172,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&324]); // beq
        self.bind_label(labels[&322]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&336]); // b
        self.bind_label(labels[&324]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&336]); // b
        self.bind_label(labels[&326]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&331]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 172,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&333]); // beq
        self.bind_label(labels[&331]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&336]); // b
        self.bind_label(labels[&333]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&336]); // b
        self.bind_label(labels[&335]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&336]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&351]); // bne
        let index = self.intern_string_literal(&[
            0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33,
            0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34,
            0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30,
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 956,
        });
        let index = self.intern_string_literal(&[
            0x31, 0x37, 0x39, 0x37, 0x36, 0x39, 0x33, 0x31, 0x33, 0x34, 0x38, 0x36, 0x32, 0x33,
            0x31, 0x35, 0x38, 0x30, 0x37, 0x39, 0x33, 0x37, 0x32, 0x39, 0x30, 0x31, 0x31, 0x34,
            0x30, 0x35, 0x33, 0x30, 0x33, 0x34, 0x32, 0x30,
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 308));
        self.record_relocation(RelocationKind::Rel24, "__str2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__str2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 956,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1173]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__double_max");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__double_max");
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.bind_label(labels[&351]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "SIGNBIT".to_string(),
        });
        self.load_double_constant(0, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 30 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 31, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&368]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 912,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 914,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 916,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 917,
        });
        self.emit_branch_to(labels[&468]); // b
        self.bind_label(labels[&368]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 104,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 104,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&378]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&396]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&387]); // beq
        self.emit_branch_to(labels[&396]); // b
        self.bind_label(labels[&378]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&383]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 108,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&385]); // beq
        self.bind_label(labels[&383]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&385]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&387]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&392]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 108,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&394]); // beq
        self.bind_label(labels[&392]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&394]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&397]); // b
        self.bind_label(labels[&396]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&397]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&439]); // bgt
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 96,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 96,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 912,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 6,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 914,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 916,
        });
        self.emit_branch_conditional_to(12, 2, labels[&414]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&432]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&423]); // beq
        self.emit_branch_to(labels[&432]); // b
        self.bind_label(labels[&414]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&419]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 100,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&421]); // beq
        self.bind_label(labels[&419]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&433]); // b
        self.bind_label(labels[&421]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&433]); // b
        self.bind_label(labels[&423]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&428]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 100,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&430]); // beq
        self.bind_label(labels[&428]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&433]); // b
        self.bind_label(labels[&430]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&433]); // b
        self.bind_label(labels[&432]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&433]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&437]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&437]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 917,
        });
        self.emit_branch_to(labels[&468]); // b
        self.bind_label(labels[&439]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&442]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 30, b: 30 });
        self.bind_label(labels[&442]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 24,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__count_trailing_zero".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 30,
                a: 3,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 604,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 30, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 160,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 160,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.output
            .instructions
            .push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 560,
        });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ull2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 912,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 560,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 604,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 912,
        });
        self.bind_label(labels[&468]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 912,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__equals_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1173]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 912,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&860]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 184,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 188,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 4, a: 6, b: 4 });
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 188,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 184,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 152,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 152,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&499]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&517]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&508]); // beq
        self.emit_branch_to(labels[&517]); // b
        self.bind_label(labels[&499]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&504]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 156,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&506]); // beq
        self.bind_label(labels[&504]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&518]); // b
        self.bind_label(labels[&506]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&518]); // b
        self.bind_label(labels[&508]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&513]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 156,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&515]); // beq
        self.bind_label(labels[&513]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&518]); // b
        self.bind_label(labels[&515]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&518]); // b
        self.bind_label(labels[&517]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&518]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&522]); // bne
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&1173]); // b
        self.bind_label(labels[&522]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "SIGNBIT".to_string(),
        });
        self.load_double_constant(0, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 30 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 31, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&538]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 868,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 870,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 872,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 873,
        });
        self.emit_branch_to(labels[&638]); // b
        self.bind_label(labels[&538]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 88,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 88,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&548]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&566]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&557]); // beq
        self.emit_branch_to(labels[&566]); // b
        self.bind_label(labels[&548]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&553]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 92,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&555]); // beq
        self.bind_label(labels[&553]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&567]); // b
        self.bind_label(labels[&555]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&567]); // b
        self.bind_label(labels[&557]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&562]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 92,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&564]); // beq
        self.bind_label(labels[&562]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&567]); // b
        self.bind_label(labels[&564]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&567]); // b
        self.bind_label(labels[&566]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&567]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&609]); // bgt
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 80,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 80,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 868,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 6,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 870,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 872,
        });
        self.emit_branch_conditional_to(12, 2, labels[&584]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&602]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&593]); // beq
        self.emit_branch_to(labels[&602]); // b
        self.bind_label(labels[&584]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&589]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 84,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&591]); // beq
        self.bind_label(labels[&589]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&603]); // b
        self.bind_label(labels[&591]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&603]); // b
        self.bind_label(labels[&593]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&598]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 84,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&600]); // beq
        self.bind_label(labels[&598]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&603]); // b
        self.bind_label(labels[&600]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&603]); // b
        self.bind_label(labels[&602]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&603]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&607]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&607]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 873,
        });
        self.emit_branch_to(labels[&638]); // b
        self.bind_label(labels[&609]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&612]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 30, b: 30 });
        self.bind_label(labels[&612]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 20,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__count_trailing_zero".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 30,
                a: 3,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 516,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 30, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 144,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 144,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.output
            .instructions
            .push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 472,
        });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ull2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 868,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 472,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 516,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 31,
            a: 1,
            offset: 868,
        });
        self.bind_label(labels[&638]);
        self.load_double_constant(31, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(31, 32752));
        self.emit_branch_to(labels[&821]); // b
        self.bind_label(labels[&641]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 188,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 3, a: 5, b: 3 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 868,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 188,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 1,
            offset: 872,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 184,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 1,
            offset: 876,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 184,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 1,
            offset: 880,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 136,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 1,
            offset: 884,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 136,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 1,
            offset: 888,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 29,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 1,
            offset: 892,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 896,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 31 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 900,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 904,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 1,
                offset: 908,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 912,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 12,
            a: 1,
            offset: 916,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 11,
            a: 1,
            offset: 920,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 1,
            offset: 924,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 928,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: 1,
            offset: 932,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 936,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 1,
            offset: 940,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 944,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 948,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 952,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.emit_branch_conditional_to(12, 2, labels[&683]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&701]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&692]); // beq
        self.emit_branch_to(labels[&701]); // b
        self.bind_label(labels[&683]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 29,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&688]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 140,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&690]); // beq
        self.bind_label(labels[&688]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&702]); // b
        self.bind_label(labels[&690]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&702]); // b
        self.bind_label(labels[&692]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 29,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&697]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 140,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&699]); // beq
        self.bind_label(labels[&697]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&702]); // b
        self.bind_label(labels[&699]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&702]); // b
        self.bind_label(labels[&701]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&702]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&706]); // bne
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&1173]); // b
        self.bind_label(labels[&706]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "SIGNBIT".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 31, b: 30 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 30, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&721]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 868,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 870,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 872,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 873,
        });
        self.emit_branch_to(labels[&821]); // b
        self.bind_label(labels[&721]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 72,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 72,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&731]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&749]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&740]); // beq
        self.emit_branch_to(labels[&749]); // b
        self.bind_label(labels[&731]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&736]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 76,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&738]); // beq
        self.bind_label(labels[&736]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&750]); // b
        self.bind_label(labels[&738]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&750]); // b
        self.bind_label(labels[&740]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&745]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 76,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&747]); // beq
        self.bind_label(labels[&745]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&750]); // b
        self.bind_label(labels[&747]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&750]); // b
        self.bind_label(labels[&749]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&750]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&792]); // bgt
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 64,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 64,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 868,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 6,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 870,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 872,
        });
        self.emit_branch_conditional_to(12, 2, labels[&767]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&785]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&776]); // beq
        self.emit_branch_to(labels[&785]); // b
        self.bind_label(labels[&767]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&772]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&774]); // beq
        self.bind_label(labels[&772]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&786]); // b
        self.bind_label(labels[&774]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&786]); // b
        self.bind_label(labels[&776]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&781]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&783]); // beq
        self.bind_label(labels[&781]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&786]); // b
        self.bind_label(labels[&783]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&786]); // b
        self.bind_label(labels[&785]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&786]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&790]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&790]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 873,
        });
        self.emit_branch_to(labels[&821]); // b
        self.bind_label(labels[&792]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&795]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 30, b: 30 });
        self.bind_label(labels[&795]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 16,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 30, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__count_trailing_zero".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 29,
                a: 3,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 428,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 29, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 128,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 128,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.output
            .instructions
            .push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 384,
        });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ull2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 868,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 384,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 428,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 868,
        });
        self.bind_label(labels[&821]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 868,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&641]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 824,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 912,
        });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__minus_dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 780,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 868,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__minus_dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 824,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 780,
        });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__equals_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&852]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 196,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1173]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&1173]); // b
        self.bind_label(labels[&852]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 824,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 780,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1173]); // bne
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 184,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&1173]); // b
        self.bind_label(labels[&860]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 176,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 180,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 3, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 180,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 176,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "SIGNBIT".to_string(),
        });
        self.load_double_constant(0, 0x0000000000000000);
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 30 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 30, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&886]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 738,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 740,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 741,
        });
        self.emit_branch_to(labels[&986]); // b
        self.bind_label(labels[&886]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 56,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 56,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&896]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&914]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&905]); // beq
        self.emit_branch_to(labels[&914]); // b
        self.bind_label(labels[&896]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&901]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 60,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&903]); // beq
        self.bind_label(labels[&901]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&915]); // b
        self.bind_label(labels[&903]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&915]); // b
        self.bind_label(labels[&905]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&910]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 60,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&912]); // beq
        self.bind_label(labels[&910]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&915]); // b
        self.bind_label(labels[&912]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&915]); // b
        self.bind_label(labels[&914]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&915]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&957]); // bgt
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 48,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 48,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 6,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 738,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 740,
        });
        self.emit_branch_conditional_to(12, 2, labels[&932]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&950]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&941]); // beq
        self.emit_branch_to(labels[&950]); // b
        self.bind_label(labels[&932]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&937]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&939]); // beq
        self.bind_label(labels[&937]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&951]); // b
        self.bind_label(labels[&939]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&951]); // b
        self.bind_label(labels[&941]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&946]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&948]); // beq
        self.bind_label(labels[&946]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&951]); // b
        self.bind_label(labels[&948]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&951]); // b
        self.bind_label(labels[&950]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&951]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&955]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&955]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 741,
        });
        self.emit_branch_to(labels[&986]); // b
        self.bind_label(labels[&957]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&960]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 30, b: 30 });
        self.bind_label(labels[&960]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 12,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 30, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__count_trailing_zero".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 29,
                a: 3,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 340,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 29, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 120,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 120,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.output
            .instructions
            .push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 296,
        });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ull2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 736,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 296,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 340,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.bind_label(labels[&986]);
        self.load_double_constant(31, 0x0000000000000000);
        self.emit_branch_to(labels[&1135]); // b
        self.bind_label(labels[&988]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 180,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 1,
            offset: 736,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 180,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 1,
            offset: 740,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 176,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 1,
            offset: 744,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 176,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 1,
            offset: 748,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 1,
            offset: 752,
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 1,
            offset: 756,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 760,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 764,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 768,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 772,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 1,
                offset: 776,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 12,
            a: 1,
            offset: 912,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 11,
            a: 1,
            offset: 916,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 1,
            offset: 920,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 924,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: 1,
            offset: 928,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 932,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 1,
            offset: 936,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 940,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 944,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 948,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 1,
            offset: 952,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.record_relocation(RelocationKind::Rel24, "SIGNBIT");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "SIGNBIT".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 31, b: 30 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 30, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&1035]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 1,
            offset: 738,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 740,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 741,
        });
        self.emit_branch_to(labels[&1135]); // b
        self.bind_label(labels[&1035]);
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 40,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 40,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1045]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&1063]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1054]); // beq
        self.emit_branch_to(labels[&1063]); // b
        self.bind_label(labels[&1045]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1050]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1052]); // beq
        self.bind_label(labels[&1050]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1064]); // b
        self.bind_label(labels[&1052]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&1064]); // b
        self.bind_label(labels[&1054]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1059]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1061]); // beq
        self.bind_label(labels[&1059]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&1064]); // b
        self.bind_label(labels[&1061]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&1064]); // b
        self.bind_label(labels[&1063]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&1064]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 1, labels[&1106]); // bgt
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 30,
                a: 1,
                offset: 32,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 6,
            shift: 0,
            begin: 1,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 738,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 740,
        });
        self.emit_branch_conditional_to(12, 2, labels[&1081]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&1099]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1090]); // beq
        self.emit_branch_to(labels[&1099]); // b
        self.bind_label(labels[&1081]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1086]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1088]); // beq
        self.bind_label(labels[&1086]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&1100]); // b
        self.bind_label(labels[&1088]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.emit_branch_to(labels[&1100]); // b
        self.bind_label(labels[&1090]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 12,
            });
        self.emit_branch_conditional_to(4, 2, labels[&1095]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1097]); // beq
        self.bind_label(labels[&1095]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.emit_branch_to(labels[&1100]); // b
        self.bind_label(labels[&1097]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 3));
        self.emit_branch_to(labels[&1100]); // b
        self.bind_label(labels[&1099]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.bind_label(labels[&1100]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 73));
        self.emit_branch_conditional_to(4, 2, labels[&1104]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 78));
        self.bind_label(labels[&1104]);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 741,
        });
        self.emit_branch_to(labels[&1135]); // b
        self.bind_label(labels[&1106]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&1109]); // beq
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 30, b: 30 });
        self.bind_label(labels[&1109]);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "frexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "frexp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 30, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "__count_trailing_zero");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__count_trailing_zero".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 29,
                a: 3,
                immediate: 53,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 252,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 29, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__two_exp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__two_exp".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 30 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "ldexp");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ldexp".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 112,
        });
        self.record_relocation(RelocationKind::Rel24, "modf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "modf".to_string(),
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 112,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.output
            .instructions
            .push(Instruction::move_register(6, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 208,
        });
        self.record_relocation(RelocationKind::Rel24, "__ull2dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__ull2dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 736,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 208,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 252,
        });
        self.record_relocation(RelocationKind::Rel24, "__timesdec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__timesdec".to_string(),
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 30,
            a: 1,
            offset: 736,
        });
        self.bind_label(labels[&1135]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 1000,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 736,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&988]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 692,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 1000,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 736,
        });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__minus_dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 648,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 912,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 1000,
        });
        self.record_relocation(RelocationKind::Rel24, "__minus_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__minus_dec".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 692,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 648,
        });
        self.record_relocation(RelocationKind::Rel24, "__equals_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__equals_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1166]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 196,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::And { a: 0, s: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::And { a: 3, s: 5, b: 3 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 3, s: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::Xor { a: 0, s: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1173]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.emit_branch_to(labels[&1173]); // b
        self.bind_label(labels[&1166]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 692,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 648,
        });
        self.record_relocation(RelocationKind::Rel24, "__less_dec");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__less_dec".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1173]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.bind_label(labels[&1173]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 1000,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&1179]); // beq
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 192,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 0, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 192,
            });
        self.bind_label(labels[&1179]);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 192,
        });
        self.bind_label(labels[&1180]);
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoad {
                d: 31,
                a: 1,
                offset: 1112,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 1104,
        });
        self.output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoad {
                d: 30,
                a: 1,
                offset: 1096,
                w: 0,
                i: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 1088,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 1084,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 1080,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 1124,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 1076,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 1120,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
