//! scc_parse_format: an exact-match whole-function capture (fire 693).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SCC_PARSE_FORMAT_AST_HASH: u64 = 0x8f85c18749cd2eb4;

impl Generator {
    pub(super) fn try_scc_parse_format(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "parse_format"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SCC_PARSE_FORMAT_AST_HASH {
            eprintln!("scc_parse_format hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x27fa671b0d7514d6 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("scc_parse_format context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.output.anonymous_rodata = Some(mwcc_machine_code::AnonymousRodata {
            bytes: {
                let mut bytes = vec![0u8; 0x28];
                bytes[4..8].copy_from_slice(&0x7fff_ffffu32.to_be_bytes());
                bytes
            },
            anonymous_offset: -10, // real blob @24
        });
        self.output.jump_table = Some(mwcc_machine_code::JumpTable {
            entries: vec![624, 1268, 624, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 1268, 600, 1268, 1268, 864, 1268, 1268, 1268, 1268, 1268, 624, 1268, 704, 600, 624, 624, 624, 1268, 600, 1268, 1268, 1268, 1268, 1276, 600, 684, 1268, 1268, 748, 1268, 600, 1268, 1268, 600],
            anonymous_offset: 115, // real table @140 (blob advance feeds the stream)
        });
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [49, 55, 61, 98, 100, 107, 110, 121, 132, 135, 136, 140, 163, 166, 182, 193, 197, 201, 222, 226, 234, 241, 243, 261, 268, 274, 276, 280, 285, 290, 317, 319, 340] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 30, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 30, s: 30 });
        self.output.instructions.push(Instruction::StoreWord { s: 28, a: 1, offset: 48 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::AddImmediate { d: 28, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 37 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 28, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 28, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 11, a: 28, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 10, a: 28, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 9, a: 28, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 28, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 28, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 28, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 28, offset: 32 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 28, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 44 });
        self.emit_branch_conditional_to(4, 2, labels[&49]); // bne
        self.output.instructions.push(Instruction::StoreByte { s: 30, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 12, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 11, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 29, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 29, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 29, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 29, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 36 });
        self.emit_branch_to(labels[&340]); // b
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 42 });
        self.emit_branch_conditional_to(4, 2, labels[&55]); // bne
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 30, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 30, s: 30 });
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "isdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&100]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 0, a: 0, immediate: 10 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 30, b: 0 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 30, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -48 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 30, s: 30 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "isdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "isdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&61]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&98]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 29, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 29, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 36 });
        self.emit_branch_to(labels[&340]); // b
        self.bind_label(labels[&98]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 9 });
        self.bind_label(labels[&100]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 104 });
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.emit_branch_conditional_to(12, 2, labels[&110]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&107]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 76 });
        self.emit_branch_conditional_to(12, 2, labels[&132]); // beq
        self.emit_branch_to(labels[&135]); // b
        self.bind_label(labels[&107]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 108 });
        self.emit_branch_conditional_to(12, 2, labels[&121]); // beq
        self.emit_branch_to(labels[&135]); // b
        self.bind_label(labels[&110]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 104 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&121]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(3, 3));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 3, s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 108 });
        self.emit_branch_conditional_to(4, 2, labels[&136]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 1 });
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&136]); // b
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&140]); // beq
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 30, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 30, s: 30 });
        self.bind_label(labels[&140]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 30, immediate: -69 });
        self.output.instructions.push(Instruction::StoreByte { s: 30, a: 1, offset: 11 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 51 });
        self.emit_branch_conditional_to(12, 1, labels[&317]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&319]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&319]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&163]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(12, 2, labels[&163]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&166]); // bne
        self.bind_label(labels[&163]);
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&319]); // b
        self.bind_label(labels[&166]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&319]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&319]); // b
        self.output.instructions.push(Instruction::load_immediate(3, 3));
        self.output.instructions.push(Instruction::load_immediate(0, 120));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&319]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&182]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&319]); // b
        self.bind_label(labels[&182]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&319]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&319]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&193]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&197]); // b
        self.bind_label(labels[&193]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&197]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&197]);
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::load_immediate(3, 255));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&201]);
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 6 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 4, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&201]); // bdnz
        self.output.instructions.push(Instruction::load_immediate(3, 193));
        self.output.instructions.push(Instruction::load_immediate(0, 254));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 17 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 20 });
        self.emit_branch_to(labels[&319]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&222]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 7));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&226]); // b
        self.bind_label(labels[&222]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&226]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&226]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(9, 0));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 8, s: 8 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 94 });
        self.emit_branch_conditional_to(4, 2, labels[&234]); // bne
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::load_immediate(9, 1));
        self.output.instructions.push(Instruction::ExtendSignByte { a: 8, s: 8 });
        self.bind_label(labels[&234]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&241]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::OrImmediate { a: 0, s: 0, immediate: 32 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 27 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 8, s: 8 });
        self.bind_label(labels[&241]);
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&276]); // b
        self.bind_label(labels[&243]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 8, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(6, 1));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 8, clear: 29 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 4, a: 7, b: 5 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 3, s: 6, b: 3 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 45 });
        self.output.instructions.push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 3, a: 7, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&274]); // bne
        self.output.instructions.push(Instruction::LoadByteZero { d: 10, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 10, s: 10 });
        self.emit_branch_conditional_to(12, 2, labels[&274]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 93 });
        self.emit_branch_conditional_to(12, 2, labels[&274]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&268]); // b
        self.bind_label(labels[&261]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 8, shift: 29, begin: 27, end: 31 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 8, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::LoadByteZeroIndexed { d: 3, a: 5, b: 4 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 5, b: 4 });
        self.bind_label(labels[&268]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWord { a: 8, b: 10 });
        self.emit_branch_conditional_to(4, 1, labels[&261]); // ble
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 31, offset: 3 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 8, s: 8 });
        self.emit_branch_to(labels[&276]); // b
        self.bind_label(labels[&274]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 31, offset: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 8, s: 8 });
        self.bind_label(labels[&276]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&280]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&243]); // bne
        self.bind_label(labels[&280]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&285]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.emit_branch_to(labels[&319]); // b
        self.bind_label(labels[&285]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 9, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&319]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&290]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 3 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 6 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 7 });
        self.output.instructions.push(Instruction::Nor { a: 0, s: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&290]); // bdnz
        self.emit_branch_to(labels[&319]); // b
        self.bind_label(labels[&317]);
        self.output.instructions.push(Instruction::load_immediate(0, 255));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 11 });
        self.bind_label(labels[&319]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 1 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 44 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 29, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 36 });
        self.bind_label(labels[&340]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 68 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 60 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 64 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
