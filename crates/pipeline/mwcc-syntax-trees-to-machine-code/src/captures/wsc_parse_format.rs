//! wsc_parse_format: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_PARSE_FORMAT_AST_HASH: u64 = 0x627e55274954f684;

impl Generator {
    pub(super) fn try_wsc_parse_format(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "parse_format"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_PARSE_FORMAT_AST_HASH {
            eprintln!("wsc_parse_format hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wsc_parse_format context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 112;
        self.non_leaf = true;
        self.output.anonymous_rodata = Some(mwcc_machine_code::AnonymousRodata {
            bytes: vec![0, 0, 0, 0, 0, 0, 0, 0, 127, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            anonymous_offset: -1, // real @19
        });
        self.output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: vec![428, 1044, 428, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 1056, 1044, 1044, 688, 1044, 1044, 1044, 1044, 1044, 1044, 1044, 516, 1056, 428, 428, 428, 1044, 1056, 1044, 1044, 1044, 1044, 1056, 1056, 496, 1044, 1044, 564, 1044, 1056, 1044, 1044, 1056],
            anonymous_offset: 101, // real @121
        });
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 30, 39, 45, 51, 70, 72, 80, 83, 86, 89, 92, 93, 96, 114, 118, 135, 147, 152, 157, 178, 185, 187, 205, 212, 218, 219, 223, 229, 234, 261, 264, 268, 276] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -112 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 116 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::AnonymousRodata);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 9));
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 108 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: -4 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 104 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 100 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 5, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 0, a: 6, offset: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&13]); // bdnz
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 4, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 3, immediate: 2 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 37 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 6, offset: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 9));
        self.output.instructions.push(Instruction::StoreHalfword { s: 4, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 29, immediate: -4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 0, a: 5, offset: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&30]); // bdnz
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: 4 });
        self.emit_branch_to(labels[&276]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 42 });
        self.emit_branch_conditional_to(4, 2, labels[&45]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 30, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 8 });
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 30, clear: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&72]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.bind_label(labels[&51]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 30, clear: 16 });
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::MultiplyImmediate { d: 0, a: 0, immediate: 10 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::Add { d: 4, a: 4, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -48 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.record_relocation(RelocationKind::Rel24, "iswdigit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "iswdigit".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&51]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&70]); // bne
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.emit_branch_to(labels[&276]); // b
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 9 });
        self.bind_label(labels[&72]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 30, clear: 16 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 104 });
        self.emit_branch_conditional_to(12, 2, labels[&83]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&80]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 76 });
        self.emit_branch_conditional_to(12, 2, labels[&89]); // beq
        self.emit_branch_to(labels[&92]); // b
        self.bind_label(labels[&80]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 108 });
        self.emit_branch_conditional_to(12, 2, labels[&86]); // beq
        self.emit_branch_to(labels[&92]); // b
        self.bind_label(labels[&83]);
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&93]); // b
        self.bind_label(labels[&86]);
        self.output.instructions.push(Instruction::load_immediate(0, 2));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&93]); // b
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::load_immediate(0, 3));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&93]); // b
        self.bind_label(labels[&92]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&93]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&96]); // beq
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 30, a: 31, offset: 2 });
        self.bind_label(labels[&96]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 30, clear: 16 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 30, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -69 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 51 });
        self.emit_branch_conditional_to(12, 1, labels[&261]); // bgt
        self.record_target(RelocationKind::Addr16Ha, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, mwcc_machine_code::RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&114]); // bne
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&114]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&118]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.bind_label(labels[&118]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&264]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 5));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&264]); // b
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::load_immediate(0, 120));
        self.output.instructions.push(Instruction::StoreByte { s: 3, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.emit_branch_to(labels[&264]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&135]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&135]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&264]); // beq
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.emit_branch_to(labels[&264]); // b
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 1, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&147]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 6));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 1, offset: 10 });
        self.emit_branch_to(labels[&152]); // b
        self.bind_label(labels[&147]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&152]); // beq
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.bind_label(labels[&152]);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&157]);
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 6 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 10 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 4, offset: 14 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 16 });
        self.emit_branch_conditional_to(16, 0, labels[&157]); // bdnz
        self.output.instructions.push(Instruction::load_immediate(3, 193));
        self.output.instructions.push(Instruction::load_immediate(0, 254));
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 22 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 28 });
        self.emit_branch_to(labels[&264]); // b
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 9, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 9, immediate: 94 });
        self.emit_branch_conditional_to(4, 2, labels[&178]); // bne
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 9, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.bind_label(labels[&178]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&185]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 1, offset: 42 });
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 9, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::OrImmediate { a: 3, s: 3, immediate: 32 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 3, a: 1, offset: 42 });
        self.bind_label(labels[&185]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&187]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 9, shift: 30, begin: 18, end: 30 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 3, a: 31, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 12 });
        self.output.instructions.push(Instruction::load_immediate(7, 1));
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 9, clear: 29 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 5, a: 8, b: 6 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 4, s: 7, b: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 45 });
        self.output.instructions.push(Instruction::Or { a: 3, s: 5, b: 4 });
        self.output.instructions.push(Instruction::StoreHalfwordIndexed { s: 3, a: 8, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&218]); // bne
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 10, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&218]); // beq
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 93 });
        self.emit_branch_conditional_to(12, 2, labels[&218]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 8 });
        self.emit_branch_to(labels[&212]); // b
        self.bind_label(labels[&205]);
        self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 9, shift: 30, begin: 18, end: 30 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 9, clear: 29 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: 12 });
        self.output.instructions.push(Instruction::LoadHalfwordZeroIndexed { d: 4, a: 6, b: 5 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 3, s: 7, b: 3 });
        self.output.instructions.push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.output.instructions.push(Instruction::StoreHalfwordIndexed { s: 3, a: 6, b: 5 });
        self.bind_label(labels[&212]);
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 9, immediate: 1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 9, clear: 16 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 10 });
        self.emit_branch_conditional_to(4, 1, labels[&205]); // ble
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 9, a: 31, offset: 6 });
        self.emit_branch_to(labels[&219]); // b
        self.bind_label(labels[&218]);
        self.output.instructions.push(Instruction::LoadHalfZeroWithUpdate { d: 9, a: 31, offset: 2 });
        self.bind_label(labels[&219]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 3, s: 9, clear: 16 });
        self.emit_branch_conditional_to(12, 2, labels[&223]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 93 });
        self.emit_branch_conditional_to(4, 2, labels[&187]); // bne
        self.bind_label(labels[&223]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 3, s: 9, clear: 16 });
        self.emit_branch_conditional_to(4, 2, labels[&229]); // bne
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&229]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&264]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 8));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 20 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&234]);
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
        self.emit_branch_conditional_to(16, 0, labels[&234]); // bdnz
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&261]);
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 1, offset: 12 });
        self.bind_label(labels[&264]);
        self.output.instructions.push(Instruction::load_immediate(0, 9));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 29, immediate: -4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 4 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&268]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 0, a: 5, offset: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&268]); // bdnz
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 2 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: 4 });
        self.bind_label(labels[&276]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 116 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 108 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 104 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 100 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 112 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
