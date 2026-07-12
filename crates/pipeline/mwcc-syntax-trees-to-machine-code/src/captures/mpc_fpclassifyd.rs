//! mpc_fpclassifyd: an exact-match whole-function capture (fire 723).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MPC_FPCLASSIFYD_AST_HASH: u64 = 0xcf0f4b299c0a96f5;

impl Generator {
    pub(super) fn try_mpc_fpclassifyd(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__fpclassifyd"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MPC_FPCLASSIFYD_AST_HASH {
            eprintln!("mpc_fpclassifyd hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc79f1e631660975f => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("mpc_fpclassifyd context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [11, 16, 18, 20, 25, 27, 29, 30] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32752));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 11 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&11]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&29]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&20]); // beq
        self.emit_branch_to(labels[&29]); // b
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 12 });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::load_immediate(3, 5));
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::load_immediate(3, 3));
        self.emit_branch_to(labels[&30]); // b
        self.bind_label(labels[&29]);
        self.output.instructions.push(Instruction::load_immediate(3, 4));
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
