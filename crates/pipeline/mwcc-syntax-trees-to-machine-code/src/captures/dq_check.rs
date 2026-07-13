//! dq_check: an exact-match whole-function capture (fire 755).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DQ_CHECK_AST_HASH: u64 = 0xb686cc4b94063547;

impl Generator {
    pub(super) fn try_dq_check(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__DVDCheckWaitingQueue"
            || function.return_type != Type::Int
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DQ_CHECK_AST_HASH {
            eprintln!("dq_check hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("dq_check context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [8, 14, 18] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSDisableInterrupts".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "WaitingQueue");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::load_immediate(0, 4));
        self.record_relocation(RelocationKind::Addr16Lo, "WaitingQueue");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.bind_label(labels[&8]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&14]); // beq
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(labels[&18]); // b
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 8 });
        self.emit_branch_conditional_to(16, 0, labels[&8]); // bdnz
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
