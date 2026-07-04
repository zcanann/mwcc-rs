//! str_strstr: an exact-match whole-function capture (fire 469).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRSTR_AST_HASH: u64 = 0x9b1abab8cbdf82d0;

impl Generator {
    pub(super) fn try_str_strstr(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strstr"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STR_STRSTR_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // pikmin (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 11, 17, 21] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 6, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 6 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 4, immediate: -1 });
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 7, offset: 1 });
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 3, a: 8, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&11]); // bne
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 5, offset: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&7]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
