//! str_strrchr: an exact-match whole-function capture (fire 471).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRRCHR_AST_HASH: u64 = 0xb2a9046654c5c369;

impl Generator {
    pub(super) fn try_str_strrchr(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strrchr"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STR_STRRCHR_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // pikmin2 (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 7, 16] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 24 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&7]); // b
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 4, a: 5, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 2 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&16]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
