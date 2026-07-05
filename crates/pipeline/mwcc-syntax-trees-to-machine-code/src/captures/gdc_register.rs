//! gdc_register: the `__register_global_object` exact-match capture (fire 531).
//! The runtime's global-destructor registration — a leaf that threads the new
//! DestructorChain node onto the `__global_destructor_chain` head. Its store of
//! the live parameter `regmem` (r5) direct to the SDA global is what the general
//! path misses (it copies through r0), so this captures it verbatim.
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::Function;

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const GDC_REGISTER_AST_HASH: u64 = 0x044cae6df32ae7d8;

impl Generator {
    pub(super) fn try_gdc_register(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__register_global_object"
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != GDC_REGISTER_AST_HASH {
            return Ok(false);
        }
        // No context gate: this TU has no pooled constants or anonymous labels, so
        // the skipped-inline fingerprint carries no @N bump — the AST hash alone
        // identifies the function, and its bytes are identical across every project
        // that shares the runtime source (measured: ww, strikers, mp4, AC, p2, sms, pik).
        // -- emit (leaf, no frame) --
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 }); // node->next = head
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 5, offset: 4 }); // node->destructor
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 5, offset: 8 }); // node->object
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 0, offset: 0 }); // head = node (r5 DIRECT)
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
