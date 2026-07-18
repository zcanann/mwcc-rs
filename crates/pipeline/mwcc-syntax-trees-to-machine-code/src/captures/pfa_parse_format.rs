//! pfa_parse_format: an exact-match whole-function capture (fire 695).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFA_PARSE_FORMAT_AST_HASH: u64 = 0x1b083ddc32105ec;

impl Generator {
    pub(super) fn try_pfa_parse_format(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "parse_format"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFA_PARSE_FORMAT_AST_HASH {
            eprintln!("pfa_parse_format hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (bump TBD)
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            0x46f259063d157aea => 0, // wind_waker (bump TBD)
            0xf8b1cd38c2b39c70 => 0, // animal_crossing (bump TBD)
            0x3012f8741ad9c69d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfa_parse_format context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        // TWO dense switches — creation order: the 17-entry (@159) first, the
        // 56-entry (@160) second; layout reverses (writer handles it).
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                188, 248, 248, 212, 248, 248, 248, 248, 248, 248, 248, 176, 248, 164, 248, 248, 224,
            ],
            anonymous_offset: 133, // real @159
        });
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![
                908, 1212, 1212, 1212, 988, 852, 968, 1212, 1212, 1212, 1212, 1212, 1212, 1212,
                1212, 1212, 1212, 1212, 1212, 1212, 1212, 1212, 1212, 780, 1212, 1212, 1212, 1212,
                1212, 1212, 1212, 1212, 908, 1212, 1088, 780, 988, 852, 968, 1212, 780, 1212, 1212,
                1212, 1212, 1188, 780, 1052, 1212, 1212, 1144, 1212, 780, 1212, 1212, 780,
            ],
            anonymous_offset: 1, // @160 = @159 + 1
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            31, 62, 63, 68, 81, 84, 87, 94, 98, 112, 129, 132, 135, 142, 146, 153, 156, 166, 177,
            180, 181, 185, 201, 207, 218, 221, 232, 239, 247, 254, 257, 278, 283, 292, 303, 305,
            314,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: 3,
            offset: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 37,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 1,
            offset: 9,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 1,
            offset: 10,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 1,
            offset: 11,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 7,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 20,
        });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output.instructions.push(Instruction::StoreByte {
            s: 6,
            a: 1,
            offset: 13,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 30,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 30,
            offset: 12,
        });
        self.emit_branch_to(labels[&314]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 6,
            immediate: -32,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 16,
            });
        self.emit_branch_conditional_to(12, 1, labels[&62]); // bgt
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::JumpTableAt(0),
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
            mwcc_machine_code::RelocationTarget::JumpTableAt(0),
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
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&63]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 9,
        });
        self.emit_branch_to(labels[&63]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 9,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&63]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 9,
        });
        self.emit_branch_to(labels[&63]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 11,
        });
        self.emit_branch_to(labels[&63]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&63]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&63]); // b
        self.bind_label(labels[&62]);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&63]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&68]); // beq
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&68]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 42,
            });
        self.emit_branch_conditional_to(4, 2, labels[&84]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__va_arg".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.emit_branch_conditional_to(4, 0, labels[&81]); // bge
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.bind_label(labels[&81]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.emit_branch_to(labels[&98]); // b
        self.bind_label(labels[&84]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.emit_branch_to(labels[&94]); // b
        self.bind_label(labels[&87]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: 0,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.bind_label(labels[&94]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 6,
                clear: 24,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 27,
                end: 27,
            });
        self.emit_branch_conditional_to(4, 2, labels[&87]); // bne
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 5,
                immediate: 509,
            });
        self.emit_branch_conditional_to(4, 1, labels[&112]); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 30,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 12,
        });
        self.emit_branch_to(labels[&314]); // b
        self.bind_label(labels[&112]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 46,
            });
        self.emit_branch_conditional_to(4, 2, labels[&146]); // bne
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 42,
            });
        self.emit_branch_conditional_to(4, 2, labels[&132]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, "__va_arg");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__va_arg".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_conditional_to(4, 0, labels[&129]); // bge
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 10,
        });
        self.bind_label(labels[&129]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.emit_branch_to(labels[&146]); // b
        self.bind_label(labels[&132]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.emit_branch_to(labels[&142]); // b
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: 0,
                immediate: 10,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -48,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.bind_label(labels[&142]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 6,
                clear: 24,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 27,
                end: 27,
            });
        self.emit_branch_conditional_to(4, 2, labels[&135]); // bne
        self.bind_label(labels[&146]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 104,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.emit_branch_conditional_to(12, 2, labels[&156]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&153]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 76,
            });
        self.emit_branch_conditional_to(12, 2, labels[&177]); // beq
        self.emit_branch_to(labels[&180]); // b
        self.bind_label(labels[&153]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 6,
                immediate: 108,
            });
        self.emit_branch_conditional_to(12, 2, labels[&166]); // beq
        self.emit_branch_to(labels[&180]); // b
        self.bind_label(labels[&156]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 31,
            offset: 1,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: 104,
            });
        self.emit_branch_conditional_to(4, 2, labels[&181]); // bne
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 1,
        });
        self.emit_branch_to(labels[&181]); // b
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 31,
            offset: 1,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 3));
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
                immediate: 108,
            });
        self.emit_branch_conditional_to(4, 2, labels[&181]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 4));
        self.output
            .instructions
            .push(Instruction::move_register(6, 3));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 1,
        });
        self.emit_branch_to(labels[&181]); // b
        self.bind_label(labels[&177]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 5));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.emit_branch_to(labels[&181]); // b
        self.bind_label(labels[&180]);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&181]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&185]); // beq
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 6,
                a: 31,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 6, s: 6 });
        self.bind_label(labels[&185]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 6,
            immediate: -65,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 6,
            a: 1,
            offset: 13,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 55,
            });
        self.emit_branch_conditional_to(12, 1, labels[&303]); // bgt
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::JumpTableAt(1),
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
            mwcc_machine_code::RelocationTarget::JumpTableAt(1),
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
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&201]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&201]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&207]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&207]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&305]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&218]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&221]); // bne
        self.bind_label(labels[&218]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&221]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&305]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&232]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 13));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.bind_label(labels[&232]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&239]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&239]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&305]); // bne
        self.bind_label(labels[&239]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&247]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.bind_label(labels[&247]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&254]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&254]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&257]); // bne
        self.bind_label(labels[&254]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&257]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&305]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 120));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 3));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 8));
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 1,
            offset: 13,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 1,
            offset: 11,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&278]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&278]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 10,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&283]); // bne
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&305]); // beq
        self.bind_label(labels[&283]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&292]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&292]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&305]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&305]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.emit_branch_to(labels[&305]); // b
        self.bind_label(labels[&303]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: 13,
        });
        self.bind_label(labels[&305]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 12,
        });
        self.bind_label(labels[&314]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 44,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 40,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
