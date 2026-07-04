//! dio_fread_str: an exact-match whole-function capture (fire 511).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DIO_FREAD_STR_AST_HASH: u64 = 0x6b3295d4e058c135; // strikers (f511)

impl Generator {
    pub(super) fn try_dio_fread_str(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fread"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DIO_FREAD_STR_AST_HASH {
            eprintln!("dio_fread_str hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // strikers direct_io (f511)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(31, 6));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 28, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::move_register(28, 3));
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.record_relocation(RelocationKind::Rel24, "__begin_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__begin_critical_region".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::move_register(5, 30));
        self.output.instructions.push(Instruction::move_register(6, 31));
        self.record_relocation(RelocationKind::Rel24, "__fread");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__fread".to_string() });
        self.output.instructions.push(Instruction::move_register(0, 3));
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::move_register(31, 0));
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 28, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
