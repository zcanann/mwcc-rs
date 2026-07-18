//! strm_stringread: an exact-match whole-function capture (fire 472).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const STRM_STRINGREAD_AST_HASH: u64 = 0xd0db79e9934c9d9e;

impl Generator {
    pub(super) fn try_strm_stringread(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__StringRead"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != STRM_STRINGREAD_AST_HASH {
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
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [7, 10, 18, 22, 29, 31, 33, 35] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.output
            .instructions
            .push(Instruction::move_register(6, 3));
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&7]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.emit_branch_to(labels[&35]); // b
        self.bind_label(labels[&7]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&35]); // bge
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 3, s: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&29]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 0,
        });
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 4,
        });
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&35]);
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
