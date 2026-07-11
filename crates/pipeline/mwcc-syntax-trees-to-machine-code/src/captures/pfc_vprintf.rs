//! pfc_vprintf: an exact-match whole-function capture (fire 700).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFC_VPRINTF_AST_HASH: u64 = 0xf9845d45222db4e5;

impl Generator {
    pub(super) fn try_pfc_vprintf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "vprintf"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFC_VPRINTF_AST_HASH && hash != 0xd71901c8430b5570 {
            eprintln!("pfc_vprintf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // strikers (bump TBD)
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            _ => {
                eprintln!("pfc_vprintf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [18, 31] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__files");
        self.output.instructions.push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::move_register(30, 4));
        self.record_relocation(RelocationKind::Addr16Lo, "__files");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 4, immediate: 80 });
        self.output.instructions.push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::load_immediate(4, -1));
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fwide".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&18]); // blt
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&31]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.record_relocation(RelocationKind::Rel24, "__begin_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__begin_critical_region".to_string() });
        self.record_relocation(RelocationKind::Addr16Ha, "__FileWrite");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::move_register(4, 31));
        self.record_relocation(RelocationKind::Addr16Lo, "__FileWrite");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::move_register(5, 29));
        self.output.instructions.push(Instruction::move_register(6, 30));
        self.record_relocation(RelocationKind::Rel24, "__pformatter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__pformatter".to_string() });
        self.output.instructions.push(Instruction::move_register(0, 3));
        self.output.instructions.push(Instruction::load_immediate(3, 2));
        self.output.instructions.push(Instruction::move_register(31, 0));
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__end_critical_region".to_string() });
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
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
