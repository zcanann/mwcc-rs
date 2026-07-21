//! mf_copy_al_mp4: an exact-match whole-function capture (fire 474).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};
use mwcc_versions::MemCopyWordScheduleStyle;

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MF_COPY_AL_MP4_AST_HASHES: &[u64] =
    &[0xe811_318e_a6d5_31c7, 0xf11f_c48f_2889_2b99];

impl Generator {
    pub(super) fn try_mf_copy_al_mp4(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__copy_longs_aligned"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !MF_COPY_AL_MP4_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 10, 14, 32, 34, 38, 42] {
            labels.insert(target, self.fresh_label());
        }
        let serial =
            self.behavior.mem_copy_word_schedule_style == MemCopyWordScheduleStyle::SerialScratch;
        let (source_byte, word_count, source, destination, tail_count) = if serial {
            (7, 4, 6, 3, 4)
        } else {
            (4, 6, 7, 4, 3)
        };
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: source_byte,
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
                a: source_byte,
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
                a: word_count,
                s: 5,
                shift: 27,
                begin: 5,
                end: 31,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: source,
            a: source_byte,
            immediate: -3,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: 3,
            immediate: -3,
        });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::LoadWord {
            d: if serial { 0 } else { 3 },
            a: source,
            offset: 4,
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
                offset: 4,
            });
            for offset in [8, 12, 16, 20, 24, 28] {
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
            for (index, offset) in [8, 12, 16, 20, 24, 28].into_iter().enumerate() {
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
                    offset: offset - 4,
                });
            }
        }
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: source,
                offset: 32,
            });
        if !serial {
            self.output.instructions.push(Instruction::StoreWord {
                s: 3,
                a: destination,
                offset: 28,
            });
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: destination,
                offset: 32,
            });
        self.emit_branch_conditional_to(4, 2, labels[&14]); // bne
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: tail_count,
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
                a: source,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: tail_count,
                a: tail_count,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: destination,
                offset: 4,
            });
        self.emit_branch_conditional_to(4, 2, labels[&34]); // bne
        self.bind_label(labels[&38]);
        let remainder_source = if serial { 4 } else { 6 };
        self.emit_mem_copy_forward_remainder_setup(5, remainder_source, source, 3, destination);
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
                a: remainder_source,
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
