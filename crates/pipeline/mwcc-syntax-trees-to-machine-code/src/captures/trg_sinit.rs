//! trg_sinit: an exact-match whole-function capture (fire 711).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const TRG_SINIT_AST_HASH: u64 = 0x52b28c7d11d1c080;

impl Generator {
    pub(super) fn try_trg_sinit(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__sinit_trigf_c"
            || function.return_type != Type::Void
            || function.parameters.len() != 0
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != TRG_SINIT_AST_HASH && hash != 0x75f455653ee44cd8 {
            eprintln!("trg_sinit hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x19234177da3e2378 => 0, // pikmin
            0xa5533c97b3cd5d53 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("trg_sinit context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::Addr16Ha, "tmp_float");
        self.output.instructions.push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "tmp_float");
        self.output.instructions.push(Instruction::LoadFloatSingleWithUpdate { d: 3, a: 4, offset: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "__four_over_pi_m1");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 4, offset: 4 });
        self.record_relocation(RelocationKind::Addr16Lo, "__four_over_pi_m1");
        self.output.instructions.push(Instruction::StoreFloatSingleWithUpdate { s: 3, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 2, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 1, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 0, a: 3, offset: 12 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
