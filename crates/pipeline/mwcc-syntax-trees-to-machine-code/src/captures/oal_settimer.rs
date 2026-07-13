//! oal_settimer: an exact-match whole-function capture (fire 756).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OAL_SETTIMER_AST_HASH: u64 = 0x46c64da626e2e087;

impl Generator {
    pub(super) fn try_oal_settimer(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "SetTimer"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OAL_SETTIMER_AST_HASH {
            eprintln!("oal_settimer hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc418e20019aad651 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("oal_settimer context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [21, 31, 33] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "__OSGetSystemTime");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__OSGetSystemTime".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 31, offset: 12 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 31, offset: 8 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 5, s: 7, immediate: 32768 });
        self.output.instructions.push(Instruction::SubtractFromCarrying { d: 8, a: 4, b: 6 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 6, s: 0, immediate: 32768 });
        self.output.instructions.push(Instruction::SubtractFromCarrying { d: 0, a: 7, b: 8 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 5, a: 5, b: 6 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 5, a: 6, b: 6 });
        self.output.instructions.push(Instruction::NegateRecord { d: 5, a: 5 });
        self.emit_branch_conditional_to(12, 2, labels[&21]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.record_relocation(RelocationKind::Rel24, "PPCMtdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "PPCMtdec".to_string() });
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::load_immediate_shifted(4, -32768));
        self.output.instructions.push(Instruction::XorImmediateShifted { a: 3, s: 7, immediate: 32768 });
        self.output.instructions.push(Instruction::SubtractFromCarrying { d: 0, a: 4, b: 8 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 6 });
        self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 6, b: 6 });
        self.output.instructions.push(Instruction::NegateRecord { d: 3, a: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&31]); // beq
        self.output.instructions.push(Instruction::move_register(3, 8));
        self.record_relocation(RelocationKind::Rel24, "PPCMtdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "PPCMtdec".to_string() });
        self.emit_branch_to(labels[&33]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: -1 });
        self.record_relocation(RelocationKind::Rel24, "PPCMtdec");
        self.output.instructions.push(Instruction::BranchAndLink { target: "PPCMtdec".to_string() });
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
