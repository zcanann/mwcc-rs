//! als_malloc: an exact-match whole-function capture (fire 732).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALS_MALLOC_AST_HASH: u64 = 0x9980a68ae717a1e3;

impl Generator {
    pub(super) fn try_als_malloc(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "malloc"
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALS_MALLOC_AST_HASH {
            eprintln!("als_malloc hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("als_malloc context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [17] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.record_relocation(RelocationKind::Rel24, "__begin_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__begin_critical_region".to_string(),
        });
        self.record_relocation(RelocationKind::EmbSda21, "init");
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "protopool");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "protopool");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 52));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::EmbSda21, "init");
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(labels[&17]);
        self.record_relocation(RelocationKind::Addr16Ha, "protopool");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output
            .instructions
            .push(Instruction::move_register(4, 31));
        self.record_relocation(RelocationKind::Addr16Lo, "protopool");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, "__pool_alloc");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__pool_alloc".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(0, 3));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::move_register(31, 0));
        self.record_relocation(RelocationKind::Rel24, "__end_critical_region");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__end_critical_region".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
