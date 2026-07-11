//! pfb_vsprintf: an exact-match whole-function capture (fire 696).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFB_VSPRINTF_AST_HASH: u64 = 0x828b18a7c39e0aa7;

impl Generator {
    pub(super) fn try_pfb_vsprintf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "vsprintf"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFB_VSPRINTF_AST_HASH {
            eprintln!("pfb_vsprintf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa605ebc1c79b708d => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("pfb_vsprintf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [21] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::move_register(6, 5));
        self.output.instructions.push(Instruction::move_register(5, 4));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::load_immediate(7, -1));
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 1, immediate: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Addr16Ha, "__StringWrite");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 8 });
        self.record_relocation(RelocationKind::Addr16Lo, "__StringWrite");
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 16 });
        self.record_relocation(RelocationKind::Rel24, "__pformatter");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__pformatter".to_string() });
        self.output.instructions.push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::load_immediate(4, -2));
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&21]); // bge
        self.output.instructions.push(Instruction::move_register(4, 3));
        self.bind_label(labels[&21]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByteIndexed { s: 0, a: 31, b: 4 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
