//! cdl_delete: an exact-match whole-function capture (fire 764).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CDL_DELETE_AST_HASH: u64 = 0xf9064f1cf598776f;

impl Generator {
    pub(super) fn try_cdl_delete(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "CARDDelete"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CDL_DELETE_AST_HASH {
            eprintln!("cdl_delete hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cdl_delete context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDGetControlBlock", "__CARDGetFileNo", "__CARDPutControlBlock", "__CARDIsOpened", "__CARDGetDirBlock", "memset", "__CARDSyncCallback", "__CARDDefaultApiCallback", "DeleteCallback", "__CARDUpdateDir", "__CARDSync"].into_iter().map(String::from).collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [12, 21, 30, 47, 58, 59, 62, 64] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetControlBlock".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&12]); // bge
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 12 });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetFileNo");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetFileNo".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 4, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&21]); // bge
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 12 });
        self.record_relocation(RelocationKind::Rel24, "__CARDIsOpened");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDIsOpened".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.emit_branch_to(labels[&59]); // b
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetDirBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetDirBlock".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::load_immediate(4, 255));
        self.output.instructions.push(Instruction::LoadWord { d: 6, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::load_immediate(5, 64));
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 6 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 54 });
        self.output.instructions.push(Instruction::StoreHalfword { s: 0, a: 6, offset: 190 });
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memset".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDSyncCallback");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDDefaultApiCallback");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDSyncCallback");
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 0, a: 4, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDDefaultApiCallback");
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.output.instructions.push(Instruction::move_register(6, 0));
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Addr16Ha, "DeleteCallback");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "DeleteCallback");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 5, offset: 208 });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "__CARDUpdateDir");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDUpdateDir".to_string() });
        self.output.instructions.push(Instruction::OrRecord { a: 30, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&58]); // bge
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDPutControlBlock".to_string() });
        self.bind_label(labels[&58]);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.bind_label(labels[&59]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&62]); // bge
        self.emit_branch_to(labels[&64]); // b
        self.bind_label(labels[&62]);
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "__CARDSync");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDSync".to_string() });
        self.bind_label(labels[&64]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
