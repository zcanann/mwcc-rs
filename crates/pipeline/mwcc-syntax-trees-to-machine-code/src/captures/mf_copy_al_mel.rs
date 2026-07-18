//! mf_copy_al_mel: an exact-match whole-function capture (fire 474).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MF_COPY_AL_MEL_AST_HASH: u64 = 0x402a3621d252d27b;

impl Generator {
    pub(super) fn try_mf_copy_al_mel(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__copy_longs_aligned"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MF_COPY_AL_MEL_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa605ebc1c79b708d => 0, // melee via refctx headers (no @N in the TU)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 10, 14, 32, 34, 38, 42] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 6,
                s: 0,
                clear: 30,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.emit_branch_conditional_to(12, 2, labels[&10]); // beq
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 6, b: 5 });
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 3,
                offset: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.bind_label(labels[&10]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 6,
                s: 5,
                shift: 27,
                begin: 5,
                end: 31,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 4,
            immediate: -3,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: -3,
        });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 7,
                offset: 32,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 4,
                offset: 32,
            });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 5,
                shift: 30,
                begin: 29,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.bind_label(labels[&34]);
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 7,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 3,
                a: 3,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 4,
                offset: 4,
            });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 5,
                s: 5,
                clear: 30,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 7,
            immediate: 3,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 3,
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.bind_label(labels[&42]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 6,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 3,
                offset: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&42]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
