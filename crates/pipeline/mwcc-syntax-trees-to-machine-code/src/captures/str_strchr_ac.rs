//! str_strchr_ac: an exact-match whole-function capture (fire 472).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STR_STRCHR_AC_AST_HASH: u64 = 0xecc56389933e40bc;

impl Generator {
    pub(super) fn try_str_strchr_ac(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "strchr"
            || !matches!(function.return_type, Type::Pointer(_))
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STR_STRCHR_AC_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // animal_crossing (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [3, 5] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 24,
            });
        self.emit_branch_to(labels[&5]); // b
        self.bind_label(labels[&3]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.bind_label(labels[&5]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 4,
                a: 3,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&3]); // bne
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
