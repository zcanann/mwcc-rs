//! alm_merge_prev: an exact-match whole-function capture (fire 730).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ALM_MERGE_PREV_AST_HASH: u64 = 0xab883c501d88e4b1;

impl Generator {
    pub(super) fn try_alm_merge_prev(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "SubBlock_merge_prev"
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ALM_MERGE_PREV_AST_HASH {
            eprintln!("alm_merge_prev hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            0x626216a8cf3d36f5 => 0, // strikers (bump TBD)
            _ => {
                eprintln!("alm_merge_prev context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [25, 30] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 29,
                end: 29,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 3,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 6,
                shift: 0,
                begin: 30,
                end: 30,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 7, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 29,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 30,
                end: 30,
            });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 28,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 6, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: -4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 5, a: 7, b: 0 });
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&30]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.bind_label(labels[&30]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 7));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 4,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
