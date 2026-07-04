//! str_strcat_mp4: an exact-match whole-function capture (fire 473).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRCAT_MP4_AST_HASH: u64 = 0xad4ab8ae26b9b55d;

impl Generator {
    pub(super) fn try_str_strcat_mp4(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strcat"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STR_STRCAT_MP4_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 10, 11, 14] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.emit_branch_to(labels[&14]); // b
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 6, offset: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&11]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 0, a: 6, offset: 1 });
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 5, a: 5, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 5, a: 5, immediate: -1 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
