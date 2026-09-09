//! Shared version policy for the MSL memory-copy captures.
//!
//! The source shapes are captured separately because their preprocessed ASTs
//! vary between MSL releases. Build-dependent instruction choices belong here,
//! rather than in another generated whole-function capture.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_versions::MemCopyRemainderMaskStyle;

impl Generator {
    /// Emit the final `count &= 3` condition used by reverse copy helpers.
    pub(super) fn emit_mem_copy_remainder_mask(&mut self, count: u32) {
        match self.behavior.mem_copy_remainder_mask_style {
            MemCopyRemainderMaskStyle::MaterializedThree => {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 3,
                });
                self.output.instructions.push(Instruction::AndRecord {
                    a: count,
                    s: count,
                    b: 0,
                });
            }
            MemCopyRemainderMaskStyle::FusedClearLeft => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediateRecord {
                        a: count,
                        s: count,
                        clear: 30,
                    });
            }
        }
    }

    /// Emit the mask plus source/destination pointer setup used by forward copy
    /// helpers. Build 53 schedules the source adjustment between `li` and `and.`.
    pub(super) fn emit_mem_copy_forward_remainder_setup(
        &mut self,
        count: u32,
        source: u32,
        source_base: u32,
        destination: u32,
        destination_base: u32,
    ) {
        if self.behavior.mem_copy_remainder_mask_style
            == MemCopyRemainderMaskStyle::MaterializedThree
        {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 3,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: source,
                a: source_base,
                immediate: 3,
            });
            self.output.instructions.push(Instruction::AndRecord {
                a: count,
                s: count,
                b: 0,
            });
        } else {
            self.emit_mem_copy_remainder_mask(count);
            self.output.instructions.push(Instruction::AddImmediate {
                d: source,
                a: source_base,
                immediate: 3,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: destination_base,
            immediate: 3,
        });
    }
}
