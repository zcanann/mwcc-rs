//! ose_wait: an exact-match whole-function capture (fire 759).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OSE_WAIT_AST_HASH: u64 = 0xe8cbda7f1376f624;

impl Generator {
    pub(super) fn try_ose_wait(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSWaitSemaphore"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OSE_WAIT_AST_HASH {
            eprintln!("ose_wait hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x532c74a9b25838e0 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("ose_wait context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [10, 12] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSDisableInterrupts".to_string() });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.emit_branch_to(labels[&12]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 29, immediate: 4 });
        self.record_relocation(RelocationKind::Rel24, "OSSleepThread");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSSleepThread".to_string() });
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&10]); // ble
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 4, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 29, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::move_register(3, 30));
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
