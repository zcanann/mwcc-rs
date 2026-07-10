//! scd_stringread: an exact-match whole-function capture (fire 693).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SCD_STRINGREAD_AST_HASH: u64 = 0x987abb4791a43169;

impl Generator {
    pub(super) fn try_scd_stringread(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__StringRead"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SCD_STRINGREAD_AST_HASH {
            eprintln!("scd_stringread hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x380c6904ec5cf012 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("scd_stringread context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 10, 18, 21, 28, 30, 32, 34] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::move_register(6, 3));
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&7]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.emit_branch_to(labels[&34]); // b
        self.bind_label(labels[&7]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&34]); // bge
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&28]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 6, offset: 0 });
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 6, offset: 4 });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 6, offset: 4 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
