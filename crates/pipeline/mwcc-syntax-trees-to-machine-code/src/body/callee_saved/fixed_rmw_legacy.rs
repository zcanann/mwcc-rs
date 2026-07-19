//! Build-163 schedules for multi-node fixed-address halfword RMW programs.
//!
//! These schedules deliberately live apart from semantic recognition and the
//! mainline latency-filling schedules. Build 163 serializes inserted fields
//! through r0, materializes reusable bank-page bases, and selects rotate masks
//! from promoted 32-bit expressions.

use super::fixed_rmw_inline_tail::DmaDirectionUpdate;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn emit_legacy_fixed_rmw_triple(
        &mut self,
        high: i16,
        low: i16,
        offsets: [i16; 3],
        address: u8,
        length: u8,
    ) {
        let page = offsets.map(|offset| offset - low);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, high));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 4,
                offset: offsets[0],
            });
        for register in 5..=7 {
            self.output.instructions.push(Instruction::AddImmediate {
                d: register,
                a: 4,
                immediate: low,
            });
        }
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 0,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: address,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 5,
            offset: page[0],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: address,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 6,
                offset: page[1],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: page[1],
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: length,
            shift: 27,
            begin: 16,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 7,
                offset: page[2],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 0,
            end: 16,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 7,
            offset: page[2],
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_legacy_fixed_rmw_seven(
        &mut self,
        high: i16,
        low: i16,
        offsets: &[i16],
        direction: u8,
        main_address: u8,
        aram_address: u8,
        length: u8,
    ) {
        let page: Vec<i16> = offsets.iter().map(|offset| offset - low).collect();
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, high));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 6,
                offset: offsets[0],
            });
        for register in [8, 9] {
            self.output.instructions.push(Instruction::AddImmediate {
                d: register,
                a: 6,
                immediate: low,
            });
        }
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 0,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: main_address,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[0],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: main_address,
                clear: 16,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 6,
            immediate: low,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 5,
                a: 6,
                offset: offsets[1],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 5,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[1],
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 6,
            immediate: low,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: aram_address,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 6,
                a: 6,
                offset: offsets[2],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 6,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 8,
            offset: page[2],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: aram_address,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 6,
                a: 9,
                offset: page[3],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 6,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 6, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 9,
            offset: page[3],
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: length,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: length,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 7,
                a: 4,
                offset: page[4],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 7,
            s: 7,
            shift: 0,
            begin: 17,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 7,
                s: direction,
                shift: 15,
                begin: 0,
                end: 16,
            });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 7,
            a: 4,
            offset: page[4],
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 7,
                a: 4,
                offset: page[5],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 7,
            s: 7,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 6, s: 7, b: 6 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 6,
            a: 4,
            offset: page[5],
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 5,
                offset: page[6],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 5,
            offset: page[6],
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_legacy_fixed_rmw_inline_tail(
        &mut self,
        high: i16,
        low: i16,
        offsets: &[i16],
        tail_offset: i16,
        direction: DmaDirectionUpdate,
        poll_begin: u8,
        poll_end: u8,
        preserve_mask: i16,
        set_bits: u16,
    ) {
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, high));
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 6,
                offset: offsets[0],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 7,
            s: 0,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 3,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 7, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[0],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 3,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 7,
                a: 6,
                offset: offsets[1],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 7,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[1],
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 4,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 6,
                offset: offsets[2],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[2],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 5,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 6,
                offset: offsets[3],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[3],
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 5,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 6,
                offset: offsets[4],
            });
        match direction {
            DmaDirectionUpdate::Clear => {
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: 4,
                    s: 4,
                    shift: 0,
                    begin: 17,
                    end: 15,
                })
            }
            DmaDirectionUpdate::Set => self.output.instructions.push(Instruction::OrImmediate {
                a: 4,
                s: 4,
                immediate: 0x8000,
            }),
        }
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 4,
            a: 6,
            offset: offsets[4],
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 6,
                offset: offsets[5],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 4, b: 3 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 3,
            a: 6,
            offset: offsets[5],
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 6,
                offset: offsets[6],
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 0,
            begin: 27,
            end: 15,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: offsets[6],
        });
        // Build 163 branches back over the page-base materialization, even
        // though `lhzu` advances the base on every trip through the loop.
        let loop_top = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 6,
            immediate: low,
        });
        self.output
            .instructions
            .push(Instruction::LoadHalfZeroWithUpdate {
                d: 0,
                a: 4,
                offset: tail_offset - low,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: poll_begin,
                end: poll_end,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: loop_top,
            });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 4,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, preserve_mask));
        self.output
            .instructions
            .push(Instruction::And { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: set_bits,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 4,
            offset: 0,
        });
    }
}
