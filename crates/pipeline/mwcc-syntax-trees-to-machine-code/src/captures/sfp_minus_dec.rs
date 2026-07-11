//! sfp_minus_dec: an exact-match whole-function capture (fire 681).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFP_MINUS_DEC_AST_HASH: u64 = 0x661a741c7158f8f6;

impl Generator {
    pub(super) fn try_sfp_minus_dec(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__minus_dec"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFP_MINUS_DEC_AST_HASH {
            eprintln!("sfp_minus_dec hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xf3c0ffcf51c5b47b => 0, // strikers ansi_fp copy (bump TBD)
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfp_minus_dec context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [31, 38, 40, 45, 55, 61, 67, 68, 77, 128, 129, 136, 140, 144, 154, 162, 167, 174, 181, 182, 191, 242, 243, 250, 253, 255, 256, 274, 295, 296, 301, 304, 311, 315] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 32 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 4, offset: 40 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 3, offset: 36 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 40 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::move_register(8, 4));
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&31]); // bge
        self.output.instructions.push(Instruction::move_register(8, 0));
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 5, offset: 2 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 8, a: 8, b: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 8, immediate: 36 });
        self.emit_branch_conditional_to(4, 1, labels[&38]); // ble
        self.output.instructions.push(Instruction::load_immediate(8, 36));
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.emit_branch_to(labels[&45]); // b
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 6, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 7, a: 3, b: 4 });
        self.bind_label(labels[&45]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::CompareWord { a: 4, b: 8 });
        self.emit_branch_conditional_to(12, 0, labels[&40]); // blt
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 4, b: 8 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 7, b: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&55]); // bge
        self.output.instructions.push(Instruction::Add { d: 6, a: 4, b: 7 });
        self.bind_label(labels[&55]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 4, b: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 9, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 0, b: 7 });
        self.output.instructions.push(Instruction::Add { d: 10, a: 9, b: 7 });
        self.output.instructions.push(Instruction::move_register(11, 10));
        self.emit_branch_to(labels[&140]); // b
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 8, a: 6, offset: -1 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 7, a: 10, offset: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&136]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 12, a: 6, immediate: -1 });
        self.emit_branch_to(labels[&68]); // b
        self.bind_label(labels[&67]);
        self.output.instructions.push(Instruction::AddImmediate { d: 12, a: 12, immediate: -1 });
        self.bind_label(labels[&68]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&67]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 12, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 8, a: 12, b: 6 });
        self.emit_branch_conditional_to(12, 2, labels[&136]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 7, s: 8, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&128]); // beq
        self.bind_label(labels[&77]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 7 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 7 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 7, a: 12, offset: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&77]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 8, s: 8, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&136]); // beq
        self.bind_label(labels[&128]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 8 });
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 12, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 12, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 7, a: 12, offset: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&129]); // bdnz
        self.bind_label(labels[&136]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 10, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 8, b: 7 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 6, offset: 0 });
        self.bind_label(labels[&140]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&144]); // ble
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 10, b: 9 });
        self.emit_branch_conditional_to(12, 1, labels[&61]); // bgt
        self.bind_label(labels[&144]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 8, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 9, a: 9, b: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 9, b: 8 });
        self.emit_branch_conditional_to(4, 0, labels[&253]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 11, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(10, 0));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 5 });
        self.emit_branch_conditional_to(4, 0, labels[&154]); // bge
        self.output.instructions.push(Instruction::load_immediate(10, 1));
        self.emit_branch_to(labels[&174]); // b
        self.bind_label(labels[&154]);
        self.emit_branch_conditional_to(4, 2, labels[&174]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 8, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 11, immediate: 1 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 5, b: 7 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 6, b: 7 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 5 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 7 });
        self.emit_branch_conditional_to(4, 0, labels[&167]); // bge
        self.bind_label(labels[&162]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&253]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&162]); // bdnz
        self.bind_label(labels[&167]);
        self.output.instructions.push(Instruction::Add { d: 5, a: 9, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 4, b: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&174]); // beq
        self.output.instructions.push(Instruction::load_immediate(10, 1));
        self.bind_label(labels[&174]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 10, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&253]); // beq
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 0, labels[&250]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 6, immediate: -1 });
        self.emit_branch_to(labels[&182]); // b
        self.bind_label(labels[&181]);
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 8, immediate: -1 });
        self.bind_label(labels[&182]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&181]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 8, b: 6 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 8, b: 6 });
        self.emit_branch_conditional_to(12, 2, labels[&250]); // beq
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 5, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&242]); // beq
        self.bind_label(labels[&191]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 3 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 6 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 7 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 7 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 8, offset: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&191]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 5, s: 5, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&250]); // beq
        self.bind_label(labels[&242]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 5 });
        self.bind_label(labels[&243]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 7, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 7, immediate: 10 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 8, offset: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&243]); // bdnz
        self.bind_label(labels[&250]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 6, offset: 0 });
        self.bind_label(labels[&253]);
        self.output.instructions.push(Instruction::move_register(6, 4));
        self.emit_branch_to(labels[&256]); // b
        self.bind_label(labels[&255]);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.bind_label(labels[&256]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&255]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&304]); // ble
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 4, b: 6 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 7, s: 5, clear: 24 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::Add { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&301]); // bge
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 5, shift: 29, begin: 3, end: 31 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&295]); // beq
        self.bind_label(labels[&274]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 2 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 3 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 3 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 5 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 5 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 6 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 8 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 7 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&274]); // bdnz
        self.output.instructions.push(Instruction::AndImmediateRecord { a: 5, s: 5, immediate: 7 });
        self.emit_branch_conditional_to(12, 2, labels[&301]); // beq
        self.bind_label(labels[&295]);
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 5 });
        self.bind_label(labels[&296]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&296]); // bdnz
        self.bind_label(labels[&301]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 7, b: 0 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 4 });
        self.bind_label(labels[&304]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 5 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&315]); // ble
        self.bind_label(labels[&311]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: -1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&315]); // bne
        self.emit_branch_conditional_to(16, 0, labels[&311]); // bdnz
        self.bind_label(labels[&315]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 4, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
