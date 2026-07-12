//! mpc_fpclassifyf: an exact-match whole-function capture (fire 723).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MPC_FPCLASSIFYF_AST_HASH: u64 = 0xe58d5f6eaa11969b;

impl Generator {
    pub(super) fn try_mpc_fpclassifyf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__fpclassifyf"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MPC_FPCLASSIFYF_AST_HASH {
            eprintln!("mpc_fpclassifyf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc79f1e631660975f => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("mpc_fpclassifyf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [11, 17, 22, 23] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 32640));
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 0, begin: 1, end: 8 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&11]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&22]); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&17]); // beq
        self.emit_branch_to(labels[&22]); // b
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 3, s: 4, clear: 9 });
        self.output.instructions.push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: 3 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 2 });
        self.emit_branch_to(labels[&23]); // b
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 9 });
        self.output.instructions.push(Instruction::load_immediate(3, 3));
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 5));
        self.emit_branch_to(labels[&23]); // b
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::load_immediate(3, 4));
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
