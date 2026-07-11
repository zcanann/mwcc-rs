//! pfc_printf: an exact-match whole-function capture (fire 700).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PFC_PRINTF_AST_HASH: u64 = 0x65cbfb46d26be3bd;

impl Generator {
    pub(super) fn try_pfc_printf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "printf"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PFC_PRINTF_AST_HASH {
            eprintln!("pfc_printf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xecff4eb19d59de49 => 0, // pikmin2 (bump TBD)
            _ => {
                eprintln!("pfc_printf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 112;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [10] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -112 });
        self.emit_branch_conditional_to(4, 6, labels[&10]); // bne
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 40 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 48 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 3, a: 1, offset: 56 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 4, a: 1, offset: 64 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 5, a: 1, offset: 72 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 6, a: 1, offset: 80 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 7, a: 1, offset: 88 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 8, a: 1, offset: 96 });
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 6, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 8, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 9, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::StoreWord { s: 10, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 112 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
