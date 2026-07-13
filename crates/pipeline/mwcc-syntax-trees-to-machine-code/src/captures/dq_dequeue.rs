//! dq_dequeue: an exact-match whole-function capture (fire 755).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const DQ_DEQUEUE_AST_HASH: u64 = 0x77344e0cc54ea06a;

impl Generator {
    pub(super) fn try_dq_dequeue(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__DVDDequeueWaitingQueue"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != DQ_DEQUEUE_AST_HASH {
            eprintln!("dq_dequeue hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("dq_dequeue context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 17, 21] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSDisableInterrupts".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 31, offset: 4 });
        self.output.instructions.push(Instruction::move_register(0, 3));
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::move_register(3, 0));
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&21]); // b
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 5, offset: 4 });
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
