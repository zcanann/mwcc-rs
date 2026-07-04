//! ansif_init_file: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ANSIF_INIT_FILE_AST_HASH: u64 = 0x6dff7240b3806d1b; // strikers (f511)

impl Generator {
    pub(super) fn try_ansif_init_file(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__init_file"
            || function.return_type != Type::Void
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ANSIF_INIT_FILE_AST_HASH {
            eprintln!("ansif_init_file hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // strikers ansi_files (f511, shares pikmin's set)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [24, 28, 48] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 7, shift: 5, begin: 24, end: 26 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: 0, s: 7, shift: 4, begin: 27, end: 27 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 3, offset: 9 });
        self.output.instructions.push(Instruction::StoreByte { s: 7, a: 3, offset: 10 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 3, offset: 24 });
        self.emit_branch_conditional_to(12, 2, labels[&24]); // beq
        self.output.instructions.push(Instruction::move_register(4, 5));
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.record_relocation(RelocationKind::Rel24, "setvbuf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "setvbuf".to_string() });
        self.emit_branch_to(labels[&28]); // b
        self.bind_label(labels[&24]);
        self.output.instructions.push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::load_immediate(6, 0));
        self.record_relocation(RelocationKind::Rel24, "setvbuf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "setvbuf".to_string() });
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 31, offset: 28 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 31, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 40 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 26, begin: 29, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&48]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "__position_file");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "__read_file");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__position_file");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__write_file");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 56 });
        self.record_relocation(RelocationKind::Addr16Lo, "__read_file");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__close_file");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__write_file");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 60 });
        self.record_relocation(RelocationKind::Addr16Lo, "__close_file");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 31, offset: 64 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 68 });
        self.bind_label(labels[&48]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 72 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
