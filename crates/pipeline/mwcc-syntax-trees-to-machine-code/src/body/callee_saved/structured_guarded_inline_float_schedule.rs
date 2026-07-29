//! Linkage-first scheduling for an inlined random float guard.
//!
//! A guarded member comparison can release its receiver before a nested integer
//! call is converted to float. Build 163 reuses that saved home for the final
//! property address, preloads the property before the call, and keeps four
//! independent floating lanes live through the conversion arithmetic.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_guarded_inline_float_compare(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(plan) = guarded_inline_float_plan(&self.output.instructions) else {
            return;
        };

        self.move_instruction_before(plan.start + 18, plan.start + 4);
        self.move_instruction_before(plan.start + 19, plan.start + 5);
        self.move_instruction_before(plan.start + 14, plan.start + 11);
        self.move_instruction_before(plan.start + 16, plan.start + 13);
        self.move_instruction_before(plan.start + 18, plan.start + 15);
        self.move_instruction_before(plan.start + 20, plan.start + 17);

        let window = &mut self.output.instructions[plan.start..plan.start + 23];
        window[0] = Instruction::LoadWord {
            d: 0,
            a: plan.guard_receiver,
            offset: plan.guard_member_offset,
        };
        window[1] = Instruction::LoadWord {
            d: 3,
            a: plan.owner,
            offset: plan.owner_member_offset,
        };
        window[2] = Instruction::CompareLogicalWord { a: 0, b: 3 };
        window[4] = Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: plan.property_pointer_offset,
        };
        window[5] = Instruction::AddImmediate {
            d: plan.property_home,
            a: 3,
            immediate: plan.property_value_offset,
        };
        let Instruction::LoadFloatDouble { a, offset, .. } = window[8] else {
            unreachable!("guarded inline conversion bias load changed after recognition")
        };
        window[8] = Instruction::LoadFloatDouble { d: 4, a, offset };
        let Instruction::LoadFloatSingle { a, offset, .. } = window[11] else {
            unreachable!("guarded inline divisor load changed after recognition")
        };
        window[11] = Instruction::LoadFloatSingle { d: 3, a, offset };
        let Instruction::LoadFloatSingle { a, offset, .. } = window[13] else {
            unreachable!("guarded inline unit load changed after recognition")
        };
        window[13] = Instruction::LoadFloatSingle { d: 2, a, offset };
        let Instruction::LoadFloatDouble { a, offset, .. } = window[14] else {
            unreachable!("guarded inline conversion image load changed after recognition")
        };
        window[14] = Instruction::LoadFloatDouble { d: 0, a, offset };
        let Instruction::LoadFloatSingle { a, offset, .. } = window[15] else {
            unreachable!("guarded inline scale load changed after recognition")
        };
        window[15] = Instruction::LoadFloatSingle { d: 1, a, offset };
        window[16] = Instruction::FloatSubtractSingle { d: 4, a: 0, b: 4 };
        window[17] = Instruction::LoadFloatSingle {
            d: 0,
            a: plan.property_home,
            offset: 0,
        };
        window[18] = Instruction::FloatDivideSingle { d: 3, a: 4, b: 3 };
        window[19] = Instruction::FloatMultiplySingle { d: 2, a: 2, c: 3 };
        window[20] = Instruction::FloatMultiplySingle { d: 1, a: 1, c: 2 };
        window[21] = Instruction::FloatCompareOrdered { a: 1, b: 0 };
    }
}

#[derive(Clone, Copy)]
struct GuardedInlineFloatPlan {
    start: usize,
    guard_receiver: u8,
    guard_member_offset: i16,
    owner: u8,
    owner_member_offset: i16,
    property_pointer_offset: i16,
    property_value_offset: i16,
    property_home: u8,
}

fn guarded_inline_float_plan(instructions: &[Instruction]) -> Option<GuardedInlineFloatPlan> {
    let start = instructions.windows(23).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord { d: 3, .. },
                Instruction::LoadWord { d: 0, .. },
                Instruction::CompareLogicalWord { a: 3, b: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::BranchAndLink { .. },
                Instruction::XorImmediateShifted { .. },
                Instruction::LoadFloatDouble { .. },
                Instruction::StoreWord { .. },
                Instruction::AddImmediateShifted { .. },
                Instruction::StoreWord { .. },
                Instruction::LoadFloatDouble { .. },
                Instruction::FloatSubtractSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::FloatDivideSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::FloatMultiplySingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::FloatMultiplySingle { .. },
                Instruction::LoadWord { d: owner_copy, a: owner_base, .. },
                Instruction::LoadWord { d: property_copy, a: property_base, .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::FloatCompareOrdered { .. },
                Instruction::BranchConditionalForward { .. },
            ] if owner_copy == owner_base
                && property_copy == property_base
                && owner_copy == property_copy
        )
    })?;
    let (
        Instruction::LoadWord {
            a: guard_receiver,
            offset: guard_member_offset,
            ..
        },
        Instruction::LoadWord {
            a: owner,
            offset: owner_member_offset,
            ..
        },
        Instruction::LoadWord {
            offset: property_pointer_offset,
            ..
        },
        Instruction::LoadFloatSingle {
            offset: property_value_offset,
            ..
        },
    ) = (
        &instructions[start],
        &instructions[start + 1],
        &instructions[start + 19],
        &instructions[start + 20],
    )
    else {
        unreachable!("guarded inline float plan changed after recognition")
    };
    Some(GuardedInlineFloatPlan {
        start,
        guard_receiver: *guard_receiver,
        guard_member_offset: *guard_member_offset,
        owner: *owner,
        owner_member_offset: *owner_member_offset,
        property_pointer_offset: *property_pointer_offset,
        property_value_offset: *property_value_offset,
        property_home: *guard_receiver,
    })
}
