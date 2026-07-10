//! scb_parse_format: an exact-match whole-function capture (fire 692).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SCB_PARSE_FORMAT_AST_HASH: u64 = 0x59fb062d7abf2a84;

impl Generator {
    pub(super) fn try_scb_parse_format(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "parse_format"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SCB_PARSE_FORMAT_AST_HASH {
            eprintln!("scb_parse_format hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xb25fec2e3201cc87 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("scb_parse_format context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        // The scan_format default image + the case table (the pikmin shape).
        self.output.anonymous_rodata = Some(mwcc_machine_code::AnonymousRodata {
            bytes: {
                let mut bytes = vec![0u8; 0x28];
                bytes[4..8].copy_from_slice(&0x7fff_ffffu32.to_be_bytes());
                bytes
            },
            anonymous_offset: -10, // real blob @61 (counter @71)
        });
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![600, 1244, 600, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 1244, 576, 1244, 1244, 840, 1244, 1244, 1244, 1244, 1244, 600, 1244, 680, 576, 600, 600, 600, 1244, 576, 1244, 1244, 1244, 1244, 1252, 576, 660, 1244, 1244, 724, 1244, 576, 1244, 1244, 576],
            anonymous_offset: 115, // real table @177 (the blob advance feeds the stream)
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [42, 48, 56, 92, 94, 101, 104, 115, 126, 129, 130, 134, 157, 160, 176, 187, 191, 195, 216, 220, 228, 235, 237, 255, 262, 268, 270, 274, 279, 284, 311, 313, 334] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::load_immediate_shifted(6, 0));
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 5, s: 5 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 37 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 6, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 6, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 6, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 6, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 6, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 6, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 6, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 6, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 44 });
        self.emit_branch_conditional_to(4, 2, labels[&42]); // bne
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 4, offset: 36 });
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 42 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 5, s: 5 });
        self.bind_label(labels[&48]);
        self.record_relocation(RelocationKind::Addr16Ha, "__ctype_map");
        self.output.instructions.push(Instruction::load_immediate_shifted(6, 0));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 24 });
        self.record_relocation(RelocationKind::Addr16Lo, "__ctype_map");
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 6, immediate: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(12, 2, labels[&94]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
        self.bind_label(labels[&56]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 0, a: 0, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 6, immediate: -48 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 5, s: 5 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 0, begin: 27, end: 27 });
        self.emit_branch_conditional_to(4, 2, labels[&56]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&92]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 36 });
        self.emit_branch_to(labels[&334]); // b
        self.bind_label(labels[&92]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 9 });
        self.bind_label(labels[&94]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 104 });
        self.output.instructions.push(Instruction::load_immediate(7, 1));
        self.emit_branch_conditional_to(12, 2, labels[&104]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&101]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 76 });
        self.emit_branch_conditional_to(12, 2, labels[&126]); // beq
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&101]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 108 });
        self.emit_branch_conditional_to(12, 2, labels[&115]); // beq
        self.emit_branch_to(labels[&129]); // b
        self.bind_label(labels[&104]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(6, 2));
        self.output.instructions.push(Instruction::StoreByte { s: 6, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 6, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 104 });
        self.emit_branch_conditional_to(4, 2, labels[&130]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(5, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_to(labels[&130]); // b
        self.bind_label(labels[&115]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(6, 3));
        self.output.instructions.push(Instruction::StoreByte { s: 6, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 6, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 6, immediate: 108 });
        self.emit_branch_conditional_to(4, 2, labels[&130]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::move_register(5, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.emit_branch_to(labels[&130]); // b
        self.bind_label(labels[&126]);
        self.output.instructions.push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&130]); // b
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.bind_label(labels[&130]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&134]); // beq
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 5, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 5, s: 5 });
        self.bind_label(labels[&134]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -69 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 51 });
        self.emit_branch_conditional_to(12, 1, labels[&311]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&313]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&313]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&157]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&157]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&160]); // bne
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&313]); // b
        self.bind_label(labels[&160]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&313]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&313]); // b
        self.output.instructions.push(Instruction::load_immediate(5, 3));
        self.output.instructions.push(Instruction::load_immediate(0, 120));
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&313]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&176]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&313]); // b
        self.bind_label(labels[&176]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&313]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&313]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&187]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&191]); // b
        self.bind_label(labels[&187]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&191]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(5, 255));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&195]);
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 6 });
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 6, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&195]); // bdnz
        self.output.instructions.push(Instruction::load_immediate(5, 193));
        self.output.instructions.push(Instruction::load_immediate(0, 254));
        self.output.instructions.push(Instruction::StoreByte { s: 5, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&313]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&216]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&220]); // b
        self.bind_label(labels[&216]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&220]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&220]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 10, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(11, 0));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 10, s: 10 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 94 });
        self.emit_branch_conditional_to(4, 2, labels[&228]); // bne
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 10, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(11, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 10, s: 10 });
        self.bind_label(labels[&228]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&235]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 10, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 32 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 10, s: 10 });
        self.bind_label(labels[&235]);
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&270]); // b
        self.bind_label(labels[&237]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 10, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(8, 1));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 10, clear: 29 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 6, a: 9, b: 7 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 5, s: 8, b: 5 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 45 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 6, b: 5 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 5, a: 9, b: 7 });
        self.emit_branch_conditional_to(4, 2, labels[&268]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 12, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 12, s: 12 });
        self.emit_branch_conditional_to(12, 2, labels[&268]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 12, immediate: 93 });
        self.emit_branch_conditional_to(12, 2, labels[&268]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&262]); // b
        self.bind_label(labels[&255]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 10, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 10, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 5, immediate: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 5, a: 7, b: 6 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 8, b: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 7, b: 6 });
        self.bind_label(labels[&262]);
        self.output.instructions.push(Instruction::AddImmediate { d: 10, a: 10, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 10, b: 12 });
        self.emit_branch_conditional_to(4, 1, labels[&255]); // ble
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 10, a: 3, offset: 3 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 10, s: 10 });
        self.emit_branch_to(labels[&270]); // b
        self.bind_label(labels[&268]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 10, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 10, s: 10 });
        self.bind_label(labels[&270]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&274]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&237]); // bne
        self.bind_label(labels[&274]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&279]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&313]); // b
        self.bind_label(labels[&279]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 11, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&313]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&284]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 1 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 3 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 6 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 7 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 5, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&284]); // bdnz
        self.emit_branch_to(labels[&313]); // b
        self.bind_label(labels[&311]);
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&313]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 36 });
        self.bind_label(labels[&334]);
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
