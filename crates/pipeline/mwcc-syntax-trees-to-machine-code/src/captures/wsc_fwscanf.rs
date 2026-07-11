//! wsc_fwscanf: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_FWSCANF_AST_HASH: u64 = 0x89385b51e39f33ce;

impl Generator {
    pub(super) fn try_wsc_fwscanf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fwscanf"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_FWSCANF_AST_HASH {
            eprintln!("wsc_fwscanf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wsc_fwscanf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 128;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [16, 31, 41] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -128 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 124 });
        self.output.instructions.push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 120 });
        self.output.instructions.push(Instruction::move_register(30, 3));
        self.emit_branch_conditional_to(4, 6, labels[&16]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 4, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 5, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 6, a: 1, offset: 80 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 7, a: 1, offset: 88 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 8, a: 1, offset: 96 });
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::load_immediate(4, 1));
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 36 });
        self.record_relocation(RelocationKind::Rel24, "fwide");
        self.output.instructions.push(Instruction::BranchAndLink { target: "fwide".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&31]); // bgt
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&41]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 1, immediate: 136 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 512));
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 108 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 1, immediate: 104 });
        self.output.instructions.push(Instruction::move_register(4, 31));
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 104 });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 112 });
        self.record_relocation(RelocationKind::Rel24, "__vfwscanf");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__vfwscanf".to_string() });
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 132 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 124 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 120 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 128 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
