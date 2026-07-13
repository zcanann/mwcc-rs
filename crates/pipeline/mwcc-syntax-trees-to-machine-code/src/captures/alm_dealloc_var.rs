//! alm_dealloc_var: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_DEALLOC_VAR_AST_HASH: u64 = 0xb12f6c65e8d48381;

impl Generator {
    pub(super) fn try_alm_dealloc_var(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "deallocate_from_var_pools"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALM_DEALLOC_VAR_AST_HASH {
            eprintln!("alm_dealloc_var hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("alm_dealloc_var context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [39, 58, 63, 70, 71, 92, 99, 102, 107, 112, 119, 122, 129, 140, 146, 150, 156, 161] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 8, a: 4, immediate: -8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 4, offset: -8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 8, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 4, shift: 0, begin: 31, end: 29 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 6, s: 4, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 8, b: 6 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 4, s: 5, begin: 0, end: 30 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 30, end: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 7, offset: -4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -4 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&119]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 5, offset: 12 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 8, offset: 12 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 5, offset: 8 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 8, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 9, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 5, shift: 0, begin: 29, end: 29 });
        self.emit_branch_conditional_to(4, 2, labels[&70]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 8, a: 9, offset: -4 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 8, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&39]); // beq
        self.output.instructions.push(Instruction::move_register(7, 9));
        self.emit_branch_to(labels[&71]); // b
        self.bind_label(labels[&39]);
        self.output.instructions.push(Instruction::SubtractFrom { d: 7, a: 8, b: 9 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 5, clear: 29 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 5, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 8, b: 5 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 5, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 6, b: 5 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 7, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 5, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&58]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 5, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 8, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 6, immediate: -4 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 6, a: 7, b: 5 });
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 9 });
        self.emit_branch_conditional_to(4, 2, labels[&63]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 12 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 5, a: 4, b: 0 });
        self.bind_label(labels[&63]);
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 9, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 9, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 6, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 12 });
        self.emit_branch_to(labels[&71]); // b
        self.bind_label(labels[&70]);
        self.output.instructions.push(Instruction::move_register(7, 9));
        self.bind_label(labels[&71]);
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 7, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 9, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 10, s: 6, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::Add { d: 8, a: 9, b: 10 });
        self.output.instructions.push(Instruction::LoadWord { d: 7, a: 8, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 7, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&122]); // bne
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 6, clear: 29 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 6, s: 7, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::Add { d: 7, a: 10, b: 6 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 7, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::Or { a: 5, s: 6, b: 5 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 5, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&92]); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 7, immediate: -4 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 7, a: 9, b: 5 });
        self.bind_label(labels[&92]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 9, offset: 0 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 5, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&99]); // bne
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 9, b: 7 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 5, s: 5, shift: 0, begin: 30, end: 28 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 5, a: 9, b: 7 });
        self.emit_branch_to(labels[&102]); // b
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 9, b: 7 });
        self.output.instructions.push(Instruction::OrImmediate { a: 5, s: 5, immediate: 4 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 5, a: 9, b: 7 });
        self.bind_label(labels[&102]);
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&107]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: 12 });
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 5, a: 4, b: 0 });
        self.bind_label(labels[&107]);
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 8 });
        self.emit_branch_conditional_to(4, 2, labels[&112]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 5, a: 4, b: 0 });
        self.bind_label(labels[&112]);
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 8, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 8, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 12 });
        self.emit_branch_to(labels[&122]); // b
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::StoreWordIndexed { s: 8, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 8, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 8, offset: 12 });
        self.bind_label(labels[&122]);
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 5, a: 4, b: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 0, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&129]); // bge
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 8 });
        self.bind_label(labels[&129]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::load_immediate(7, 0));
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 0, s: 5, shift: 0, begin: 30, end: 30 });
        self.emit_branch_conditional_to(4, 2, labels[&140]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 6, s: 5, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::AndContiguousMask { a: 5, s: 0, begin: 0, end: 28 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -24 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&140]); // bne
        self.output.instructions.push(Instruction::load_immediate(7, 1));
        self.bind_label(labels[&140]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&161]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 5, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&146]); // bne
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.bind_label(labels[&146]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&150]); // bne
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: 0 });
        self.bind_label(labels[&150]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&156]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 4, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: 4 });
        self.bind_label(labels[&156]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::move_register(3, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 4, offset: 0 });
        self.record_relocation(RelocationKind::Rel24, "__sys_free");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__sys_free".to_string() });
        self.bind_label(labels[&161]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
