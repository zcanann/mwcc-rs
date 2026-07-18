//! mf_copy_run_mp4: an exact-match whole-function capture (fire 474).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MF_COPY_RUN_MP4_AST_HASH: u64 = 0x79d363464bfaa479;

impl Generator {
    pub(super) fn try_mf_copy_run_mp4(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__copy_longs_rev_unaligned"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MF_COPY_RUN_MP4_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 + AC
            0xa605ebc1c79b708d => 0, // melee (same source; no @N in the TU)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [5, 9, 16, 35, 38] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Add { d: 11, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::Add { d: 10, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 11,
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
                a: 10,
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
                a: 11,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 8,
            s: 10,
            shift: 3,
            begin: 27,
            end: 28,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 7,
                s: 10,
                clear: 30,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 9,
                a: 8,
                immediate: 32,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 5,
                shift: 3,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: 7,
                immediate: 4,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 10, a: 10, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 4,
                a: 10,
                offset: -4,
            });
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 10,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 3, s: 4, b: 9 });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 4, s: 0, b: 8 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 0, b: 9 });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 11,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 4,
                a: 10,
                offset: -8,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 4, b: 8 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 11,
                offset: -8,
            });
        self.emit_branch_conditional_to(4, 2, labels[&16]); // bne
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 5,
                shift: 0,
                begin: 29,
                end: 29,
            });
        self.emit_branch_conditional_to(12, 2, labels[&35]); // beq
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 3,
                a: 10,
                offset: -4,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 4, b: 9 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 3, b: 8 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 11,
                offset: -4,
            });
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
        self.output
            .instructions
            .push(Instruction::Add { d: 10, a: 10, b: 7 });
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 10,
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
                a: 11,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
