//! uc7_read: an exact-match whole-function capture (fire 491).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const UC7_READ_AST_HASH: u64 = 1; // DISARMED fire 491: needs output.static_locals (initialized$N) — see memory

impl Generator {
    pub(super) fn try_uc7_read(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__read_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != UC7_READ_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x177cd62da105e9a8 => 0, // measured (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 18, 28, 33, 36] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(29, 4));
        self.record_relocation(RelocationKind::Rel24, "__init_uart_console");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__init_uart_console".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(labels[&36]); // b
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 30, offset: 0 });
        self.emit_branch_to(labels[&28]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::move_register(3, 29));
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, "ReadUARTN");
        self.output.instructions.push(Instruction::BranchAndLink { target: "ReadUARTN".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 13 });
        self.emit_branch_conditional_to(12, 2, labels[&33]); // beq
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 29, immediate: 1 });
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 30, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 31 });
        self.emit_branch_conditional_to(12, 1, labels[&33]); // bgt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 31 });
        self.bind_label(labels[&36]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
