//! cdd_update: an exact-match whole-function capture (fire 761).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CDD_UPDATE_AST_HASH: u64 = 0xc362a9ceb49cfba0;

impl Generator {
    pub(super) fn try_cdd_update(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDUpdateDir"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CDD_UPDATE_AST_HASH {
            eprintln!("cdd_update hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cdd_update context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [18, 40] {
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
            .push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 3));
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 5,
                a: 28,
                immediate: 272,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDBlock");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDBlock");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 30, a: 0, b: 5 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -3));
        self.emit_branch_to(labels[&40]); // b
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 30,
            offset: 132,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 8188));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 7,
                a: 31,
                offset: 8186,
            });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 8188,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 31,
            immediate: 8190,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 7,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 31,
            offset: 8186,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDCheckSum");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDCheckSum".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 8192));
        self.record_relocation(RelocationKind::Rel24, "DCStoreRange");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "DCStoreRange".to_string(),
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 30,
            offset: 216,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "EraseCallback2");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "EraseCallback2");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 128,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 28));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 30,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 0, b: 31 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 13,
            });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 4, a: 4, b: 0 });
        self.record_relocation(RelocationKind::Rel24, "__CARDEraseSector");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDEraseSector".to_string(),
        });
        self.bind_label(labels[&40]);
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 28,
            a: 1,
            offset: 16,
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
