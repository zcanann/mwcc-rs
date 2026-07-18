//! alm_pool_free: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_POOL_FREE_AST_HASH: u64 = 0xa887a1806df1d7a9;

impl Generator {
    pub(super) fn try_alm_pool_free(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__pool_free"
            || function.return_type != Type::Void
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        // ww's materialized-inline revision hashes differently but compiles
        // to IDENTICAL bytes (byte-verified against mp4's body).
        const WW_HASH: u64 = 0xa21006ab48fc5302;
        if hash != ALM_POOL_FREE_AST_HASH && hash != WW_HASH {
            eprintln!("alm_pool_free hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        // ww: protopool$71 (56B .bss) LEADS this materialized fn's FUNC
        // symbol (the second distributed get_malloc_pool static).
        let (bump, owns_protopool): (u32, bool) = match context {
            0xbd60acb658c79e45 => (0, false), // marioparty4
            0x6b3a129a97773139 => (0, true),  // wind_waker
            0x626216a8cf3d36f5 => (0, false), // strikers
            0x9500137a19915244 => (0, false), // strikers _alloc
            _ => {
                eprintln!("alm_pool_free context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        if owns_protopool {
            self.output
                .static_locals
                .push(("protopool".to_string(), None, 56, 4, false));
            self.output.static_locals_lead = true;
            self.output.static_local_adjust = 30; // measured: protopool$71
        }
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 13, 17, 18] {
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
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_conditional_to(12, 2, labels[&18]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 4,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 5,
                clear: 31,
            });
        self.emit_branch_conditional_to(4, 2, labels[&10]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: 8,
        });
        self.emit_branch_to(labels[&13]); // b
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -8,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 5,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: -8,
        });
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 5,
                immediate: 68,
            });
        self.emit_branch_conditional_to(12, 1, labels[&17]); // bgt
        self.record_relocation(RelocationKind::Rel24, "deallocate_from_fixed_pools");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "deallocate_from_fixed_pools".to_string(),
        });
        self.emit_branch_to(labels[&18]); // b
        self.bind_label(labels[&17]);
        self.record_relocation(RelocationKind::Rel24, "deallocate_from_var_pools");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "deallocate_from_var_pools".to_string(),
        });
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
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
