//! Generation-specific emission for the fdlibm-style `frexp` transaction.
//!
//! Recognition remains in `frame`: it proves the source has the complete
//! exponent-pointer, disjunction, writeback, and mantissa-rebuild semantics.
//! This module owns build 163's already-physical schedule, which is materially
//! different from the compact virtual-register 2.4.x family.

use crate::generator::{Generator, FLOAT_SCRATCH, GENERAL_SCRATCH};
use mwcc_machine_code::Instruction;
use mwcc_target::Eabi;

pub(crate) struct FrexpFamilyPlan {
    pub(crate) eptr_register: u8,
    pub(crate) guard_high: i16,
    pub(crate) block_high: i16,
    pub(crate) scale_bits: u64,
    pub(crate) store_constant: i16,
    pub(crate) shift: u8,
    pub(crate) bias: i16,
    pub(crate) mask_begin: u8,
    pub(crate) mask_end: u8,
    pub(crate) or_high: u16,
}

impl Generator {
    /// Emit build 163's padded, physical-register frexp transaction. The plan is
    /// accepted only after the generic recognizer has verified every source-level
    /// operation, so these fixed homes describe a schedule rather than a capture.
    pub(crate) fn emit_legacy_frexp_family(&mut self, plan: FrexpFamilyPlan) {
        const SLOT: i16 = 8;
        const ZERO_OR_IX: u8 = 4;
        const HX_OR_EXPONENT: u8 = 5;
        const INITIAL_IX: u8 = 6;
        const HX: u8 = 7;
        const LX: u8 = 8;

        self.frame_size = 24;
        self.non_leaf = false;
        let float_result = Eabi::float_result().number;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(
                GENERAL_SCRATCH,
                plan.guard_high,
            ));
        self.output
            .instructions
            .push(Instruction::load_immediate(ZERO_OR_IX, 0));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: Eabi::FIRST_FLOAT_ARGUMENT,
                a: 1,
                offset: SLOT,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: HX_OR_EXPONENT,
            a: 1,
            offset: SLOT,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: LX,
            a: 1,
            offset: SLOT + 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: INITIAL_IX,
                s: HX_OR_EXPONENT,
                clear: 1,
            });
        self.output.instructions.push(Instruction::CompareWord {
            a: INITIAL_IX,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: ZERO_OR_IX,
            a: plan.eptr_register,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: HX,
            a: HX_OR_EXPONENT,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: ZERO_OR_IX,
            a: INITIAL_IX,
            immediate: 0,
        });

        let value_label = self.fresh_label();
        let skip_label = self.fresh_label();
        let merge = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, value_label);
        self.output.instructions.push(Instruction::OrRecord {
            a: GENERAL_SCRATCH,
            s: ZERO_OR_IX,
            b: LX,
        });
        self.emit_branch_conditional_to(4, 2, skip_label);
        self.bind_label(value_label);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: float_result,
            a: 1,
            offset: SLOT,
        });
        self.emit_branch_to(epilogue);

        self.bind_label(skip_label);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(
                GENERAL_SCRATCH,
                plan.block_high,
            ));
        self.output.instructions.push(Instruction::CompareWord {
            a: ZERO_OR_IX,
            b: GENERAL_SCRATCH,
        });
        self.emit_branch_conditional_to(4, 0, merge);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: float_result,
            a: 1,
            offset: SLOT,
        });
        self.output.instructions.push(Instruction::load_immediate(
            GENERAL_SCRATCH,
            plan.store_constant,
        ));
        self.load_double_constant(FLOAT_SCRATCH, plan.scale_bits);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble {
                d: FLOAT_SCRATCH,
                a: float_result,
                c: FLOAT_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: FLOAT_SCRATCH,
                a: 1,
                offset: SLOT,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: ZERO_OR_IX,
            a: 1,
            offset: SLOT,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: plan.eptr_register,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: HX,
            a: ZERO_OR_IX,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: ZERO_OR_IX,
                s: ZERO_OR_IX,
                clear: 1,
            });

        self.bind_label(merge);
        self.output.instructions.push(Instruction::LoadWord {
            d: HX_OR_EXPONENT,
            a: plan.eptr_register,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: ZERO_OR_IX,
                s: ZERO_OR_IX,
                shift: plan.shift,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: GENERAL_SCRATCH,
            s: HX,
            shift: 0,
            begin: plan.mask_begin,
            end: plan.mask_end,
        });
        self.output.instructions.push(Instruction::Add {
            d: ZERO_OR_IX,
            a: ZERO_OR_IX,
            b: HX_OR_EXPONENT,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: ZERO_OR_IX,
            a: ZERO_OR_IX,
            immediate: -plan.bias,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: ZERO_OR_IX,
            a: plan.eptr_register,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                immediate: plan.or_high,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: 1,
            offset: SLOT,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: float_result,
            a: 1,
            offset: SLOT,
        });
        self.bind_label(epilogue);
        // Build 163 assigns three additional internal block labels before the
        // pooled scale constant (@11 versus the compact family's @8). Its
        // deferred pass retains five more hidden labels.
        self.output.anonymous_label_bump += 9 + u32::from(self.behavior.frexp_deferred_label_bump);
        self.emit_epilogue_and_return();
    }
}
