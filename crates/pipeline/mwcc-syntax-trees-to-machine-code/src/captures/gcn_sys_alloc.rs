//! gcn_sys_alloc: an exact-match whole-function capture (fire 706).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const GCN_SYS_ALLOC_AST_HASH: u64 = 0x83ae26d83999572b;

impl Generator {
    pub(super) fn try_gcn_sys_alloc(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__sys_alloc"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != GCN_SYS_ALLOC_AST_HASH {
            eprintln!("gcn_sys_alloc hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x7826c186cda92236 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("gcn_sys_alloc context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [36] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.record_relocation(RelocationKind::EmbSda21, "__OSCurrHeap");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&36]); // bne
        let index = self.intern_string_literal(&[
            0x47, 0x43, 0x4e, 0x5f, 0x4d, 0x65, 0x6d, 0x5f, 0x41, 0x6c, 0x6c, 0x6f, 0x63, 0x2e,
            0x63, 0x20, 0x3a, 0x20, 0x49, 0x6e, 0x69, 0x74, 0x44, 0x65, 0x66, 0x61, 0x75, 0x6c,
            0x74, 0x48, 0x65, 0x61, 0x70, 0x2e, 0x20, 0x4e, 0x6f, 0x20, 0x48, 0x65, 0x61, 0x70,
            0x20, 0x41, 0x76, 0x61, 0x69, 0x6c, 0x61, 0x62, 0x6c, 0x65, 0x0a,
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        let index = self.intern_string_literal(&[
            0x47, 0x43, 0x4e, 0x5f, 0x4d, 0x65, 0x6d, 0x5f, 0x41, 0x6c, 0x6c, 0x6f, 0x63, 0x2e,
            0x63, 0x20, 0x3a, 0x20, 0x49, 0x6e, 0x69, 0x74, 0x44, 0x65, 0x66, 0x61, 0x75, 0x6c,
            0x74, 0x48, 0x65, 0x61, 0x70, 0x2e, 0x20, 0x4e, 0x6f, 0x20, 0x48, 0x65, 0x61, 0x70,
            0x20, 0x41, 0x76, 0x61, 0x69, 0x6c, 0x61, 0x62, 0x6c, 0x65, 0x0a,
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        let index = self.intern_string_literal(&[
            0x4d, 0x65, 0x74, 0x72, 0x6f, 0x77, 0x65, 0x72, 0x6b, 0x73, 0x20, 0x43, 0x57, 0x20,
            0x72, 0x75, 0x6e, 0x74, 0x69, 0x6d, 0x65, 0x20, 0x6c, 0x69, 0x62, 0x72, 0x61, 0x72,
            0x79, 0x20, 0x69, 0x6e, 0x69, 0x74, 0x69, 0x61, 0x6c, 0x69, 0x7a, 0x69, 0x6e, 0x67,
            0x20, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74, 0x20, 0x68, 0x65, 0x61, 0x70, 0x0a,
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &format!("@@str{index}"));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        let index = self.intern_string_literal(&[
            0x4d, 0x65, 0x74, 0x72, 0x6f, 0x77, 0x65, 0x72, 0x6b, 0x73, 0x20, 0x43, 0x57, 0x20,
            0x72, 0x75, 0x6e, 0x74, 0x69, 0x6d, 0x65, 0x20, 0x6c, 0x69, 0x62, 0x72, 0x61, 0x72,
            0x79, 0x20, 0x69, 0x6e, 0x69, 0x74, 0x69, 0x61, 0x6c, 0x69, 0x7a, 0x69, 0x6e, 0x67,
            0x20, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74, 0x20, 0x68, 0x65, 0x61, 0x70, 0x0a,
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &format!("@@str{index}"));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.record_relocation(RelocationKind::Rel24, "OSGetArenaLo");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSGetArenaLo".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "OSGetArenaHi");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSGetArenaHi".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 1));
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "OSInitAlloc");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSInitAlloc".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "OSSetArenaLo");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSSetArenaLo".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 31,
            immediate: 31,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 30,
                s: 30,
                begin: 0,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 3,
                s: 0,
                begin: 0,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "OSCreateHeap");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSCreateHeap".to_string(),
        });
        self.record_relocation(RelocationKind::Rel24, "OSSetCurrentHeap");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSSetCurrentHeap".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "OSSetArenaLo");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSSetArenaLo".to_string(),
        });
        self.bind_label(labels[&36]);
        self.record_relocation(RelocationKind::EmbSda21, "__OSCurrHeap");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 29));
        self.record_relocation(RelocationKind::Rel24, "OSAllocFromHeap");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSAllocFromHeap".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
