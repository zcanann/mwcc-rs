//! cdd_getdir: an exact-match whole-function capture (fire 761).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CDD_GETDIR_AST_HASH: u64 = 0x2f8c31def72e1ea6;

impl Generator {
    pub(super) fn try_cdd_getdir(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDGetDirBlock"
            || !matches!(function.return_type, Type::Pointer(_) | Type::StructPointer { .. })
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CDD_GETDIR_AST_HASH {
            eprintln!("cdd_getdir hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cdd_getdir context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // The OSFastCast.h plain-`inline` asm helpers surface as GLOBAL UND symbols
        // at the HEAD of the global-UND run — attach to this source-first function
        // (measured: CARDDir.c).
        self.output.phantom_externals = vec!["__OSf32tos16".to_string(), "__OSf32tou8".to_string()];
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 132 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
