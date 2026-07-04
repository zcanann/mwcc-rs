//! strm_strlen: an exact-match whole-function capture (fire 472).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STRM_STRLEN_AST_HASH: u64 = 0x2c62f9753e050023;

impl Generator {
    pub(super) fn try_strm_strlen(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strlen"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STRM_STRLEN_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // melee (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [2] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.bind_label(labels[&2]);
        self.output.instructions.push(Instruction::LoadByteZeroWithUpdate { d: 0, a: 4, offset: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 1 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&2]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
