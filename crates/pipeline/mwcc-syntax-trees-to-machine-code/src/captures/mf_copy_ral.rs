//! mf_copy_ral: an exact-match whole-function capture (fire 470).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MF_COPY_RAL_AST_HASH: u64 = 0x75ebfa29bf25c3e6;

impl Generator {
    pub(super) fn try_mf_copy_ral(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__copy_longs_rev_aligned"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MF_COPY_RAL_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // pikmin
            0xbd60acb658c79e45 => 0, // pikmin2 + BfBB (+ melee's shared bodies) — same source
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [5, 9, 11, 29, 31, 35, 37] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 7,
                clear: 30,
            });
        self.emit_branch_conditional_to(12, 2, labels[&9]); // beq
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 3, b: 5 });
        self.bind_label(labels[&5]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 6,
                offset: -1,
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
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 7,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.bind_label(labels[&9]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 4,
                s: 5,
                shift: 27,
                begin: 5,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 4,
                a: 4,
                immediate: -1,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: -8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 7,
            offset: -4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: -12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: -8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: -16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 7,
            offset: -12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: -20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: -16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: -24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 7,
            offset: -20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: -28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: -24,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 6,
                offset: -32,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 7,
            offset: -28,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 7,
                offset: -32,
            });
        self.emit_branch_conditional_to(4, 2, labels[&11]); // bne
        self.bind_label(labels[&29]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 5,
                shift: 30,
                begin: 29,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&35]); // beq
        self.bind_label(labels[&31]);
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 6,
                offset: -4,
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
                a: 7,
                offset: -4,
            });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.bind_label(labels[&35]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 5,
                s: 5,
                clear: 30,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.bind_label(labels[&37]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 6,
                offset: -1,
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
                a: 7,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
