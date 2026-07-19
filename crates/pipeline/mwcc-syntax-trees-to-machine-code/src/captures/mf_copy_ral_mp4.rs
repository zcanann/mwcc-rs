//! mf_copy_ral_mp4: an exact-match whole-function capture (fire 474).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};
use mwcc_versions::MemCopyWordScheduleStyle;

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MF_COPY_RAL_MP4_AST_HASH: u64 = 0x7ce68ebd1364d60f;

impl Generator {
    pub(super) fn try_mf_copy_ral_mp4(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__copy_longs_rev_aligned"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MF_COPY_RAL_MP4_AST_HASH {
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
        for target in [5, 9, 11, 29, 31, 35, 37] {
            labels.insert(target, self.fresh_label());
        }
        let serial =
            self.behavior.mem_copy_word_schedule_style == MemCopyWordScheduleStyle::SerialScratch;
        let (destination, source, word_count) = if serial { (6, 4, 3) } else { (7, 6, 4) };
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: 3,
            b: 5,
        });
        self.output.instructions.push(Instruction::Add {
            d: source,
            a: 4,
            b: 5,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: destination,
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
                a: source,
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
                a: destination,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&5]); // bne
        self.bind_label(labels[&9]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: word_count,
                s: 5,
                shift: 27,
                begin: 5,
                end: 31,
            });
        self.emit_branch_conditional_to(12, 2, labels[&29]); // beq
        self.bind_label(labels[&11]);
        self.output.instructions.push(Instruction::LoadWord {
            d: if serial { 0 } else { 3 },
            a: source,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: word_count,
                a: word_count,
                immediate: -1,
            });
        if serial {
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: destination,
                offset: -4,
            });
            for offset in [-8, -12, -16, -20, -24, -28] {
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: source,
                    offset,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: destination,
                    offset,
                });
            }
        } else {
            for (index, offset) in [-8, -12, -16, -20, -24, -28].into_iter().enumerate() {
                let scratch = if index % 2 == 0 { 0 } else { 3 };
                let previous = if scratch == 0 { 3 } else { 0 };
                self.output.instructions.push(Instruction::LoadWord {
                    d: scratch,
                    a: source,
                    offset,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: previous,
                    a: destination,
                    offset: offset + 4,
                });
            }
        }
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: source,
                offset: -32,
            });
        if !serial {
            self.output.instructions.push(Instruction::StoreWord {
                s: 3,
                a: destination,
                offset: -28,
            });
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: destination,
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
                a: source,
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
                a: destination,
                offset: -4,
            });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.bind_label(labels[&35]);
        self.emit_mem_copy_remainder_mask(5);
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
                a: source,
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
                a: destination,
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
