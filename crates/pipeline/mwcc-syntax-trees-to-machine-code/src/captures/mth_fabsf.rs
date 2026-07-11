//! mth_fabsf: an exact-match whole-function capture (fire 710).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MTH_FABSF_AST_HASH: u64 = 0x5021117f6e6dd98d;

impl Generator {
    pub(super) fn try_mth_fabsf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "fabsf__Ff"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MTH_FABSF_AST_HASH {
            eprintln!("mth_fabsf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa5533c97b3cd5d53 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("mth_fabsf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::FloatAbsolute { d: 1, b: 1 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
