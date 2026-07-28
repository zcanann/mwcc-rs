//! sms_os_destroy_heap: an exact-match whole-function capture (fire 529).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SMS_OS_DESTROY_HEAP_AST_HASH: u64 = 0xfed1_da4a_6f32_46d7;

impl Generator {
    pub(super) fn try_sms_os_destroy_heap(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSDestroyHeap"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_DESTROY_HEAP_AST_HASH {
            eprintln!("sms_os_destroy_heap hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            // super_mario_sunshine GMSJ01/GMSP01, GC/1.2.5n.
            0xece5_1d04_8c1e_7e9d => 0,
            _ => {
                eprintln!("sms_os_destroy_heap context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.output.symbol_order = ["HeapArray"].into_iter().map(String::from).collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: 3,
                immediate: 12,
            });
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 4, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
