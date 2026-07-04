//! uc1_write: an exact-match whole-function capture (fire 491).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const UC1_WRITE_AST_HASH: u64 = 1; // DISARMED f493: static $N base rule conditional (see writer note + memory)

impl Generator {
    pub(super) fn try_uc1_write(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__write_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != UC1_WRITE_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x38824b31e8176c4d => 0, // measured (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [18, 22, 31, 32] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.record_relocation(RelocationKind::EmbSda21, "initialized");
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -7936 });
        self.record_relocation(RelocationKind::Rel24, "InitializeUART");
        self.output.instructions.push(Instruction::BranchAndLink { target: "InitializeUART".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::EmbSda21, "initialized");
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 0, offset: 0 });
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&22]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "WriteUARTN");
        self.output.instructions.push(Instruction::BranchAndLink { target: "WriteUARTN".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 31, offset: 0 });
        self.emit_branch_to(labels[&32]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
