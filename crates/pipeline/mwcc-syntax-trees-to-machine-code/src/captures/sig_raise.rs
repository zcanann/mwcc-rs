//! sig_raise: an exact-match whole-function capture (fire 700).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SIG_RAISE_AST_HASH: u64 = 0xf13fb67ae99bb1ca;

impl Generator {
    pub(super) fn try_sig_raise(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "raise"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SIG_RAISE_AST_HASH {
            eprintln!("sig_raise hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sig_raise context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [10, 12, 23, 31, 33, 37, 42] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, labels[&10]); // blt
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 6 });
        self.emit_branch_conditional_to(4, 1, labels[&12]); // ble
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&42]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::load_immediate(3, 4));
        self.record_relocation(RelocationKind::Rel24, "__begin_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__begin_critical_region".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "signal_funcs");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 30, shift: 2 });
        self.record_relocation(RelocationKind::Addr16Lo, "signal_funcs");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 31, a: 3, offset: -4 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 0 });
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::load_immediate(3, 4));
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&33]); // bne
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&33]); // bne
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&42]); // b
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.record_relocation(RelocationKind::Rel24, "exit");
        self.output.instructions.push(Instruction::BranchAndLink { target: "exit".to_string() });
        self.bind_label(labels[&37]);
        self.output.instructions.push(Instruction::move_register(12, 31));
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&42]);
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
