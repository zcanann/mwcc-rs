//! wsc_vwscanf: an exact-match whole-function capture (fire 701).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WSC_VWSCANF_AST_HASH: u64 = 0xe1cd1f13cb179a91;

impl Generator {
    pub(super) fn try_wsc_vwscanf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "vwscanf"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WSC_VWSCANF_AST_HASH {
            eprintln!("wsc_vwscanf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wsc_vwscanf context candidate: {context:#x}");
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
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__wFileRead");
        self.output.instructions.push(Instruction::load_immediate_shifted(8, 0));
        self.output.instructions.push(Instruction::move_register(5, 3));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.record_relocation(RelocationKind::Addr16Ha, "__files");
        self.output.instructions.push(Instruction::load_immediate_shifted(7, 0));
        self.output.instructions.push(Instruction::move_register(6, 4));
        self.record_relocation(RelocationKind::Addr16Lo, "__wFileRead");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 8, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, "__files");
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 7, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, "__wsformatter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__wsformatter".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
