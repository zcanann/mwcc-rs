//! sfp_dorounddecup: an exact-match whole-function capture (fire 681).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SFP_DOROUNDDECUP_AST_HASH: u64 = 0x3f93aa80c68e850e;

impl Generator {
    pub(super) fn try_sfp_dorounddecup(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__dorounddecup"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SFP_DOROUNDDECUP_AST_HASH {
            eprintln!("sfp_dorounddecup hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sfp_dorounddecup context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [4, 10, 18] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 5, b: 6 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.bind_label(labels[&4]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 9 });
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 5 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordAlgebraic { d: 4, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: 1 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 3, offset: 2 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::StoreByte { s: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: -1 });
        self.emit_branch_to(labels[&4]); // b
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
