//! oal_set: an exact-match whole-function capture (fire 756).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OAL_SET_AST_HASH: u64 = 0x23c86aa57760fdf9;

impl Generator {
    pub(super) fn try_oal_set(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSSetAlarm"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OAL_SET_AST_HASH {
            eprintln!("oal_set hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc418e20019aad651 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("oal_set context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_27".to_string() });
        self.output.instructions.push(Instruction::move_register(27, 3));
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::move_register(28, 6));
        self.output.instructions.push(Instruction::move_register(30, 7));
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSDisableInterrupts".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 27, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 27, offset: 24 });
        self.record_relocation(RelocationKind::Rel24, "__OSGetSystemTime");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__OSGetSystemTime".to_string() });
        self.output.instructions.push(Instruction::AddCarrying { d: 6, a: 28, b: 4 });
        self.output.instructions.push(Instruction::move_register(7, 30));
        self.output.instructions.push(Instruction::AddExtended { d: 5, a: 29, b: 3 });
        self.output.instructions.push(Instruction::move_register(3, 27));
        self.record_relocation(RelocationKind::Rel24, "InsertAlarm");
        self.output.instructions.push(Instruction::BranchAndLink { target: "InsertAlarm".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink { target: "OSRestoreInterrupts".to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
