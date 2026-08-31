//! Reuse a tested halfword member and split its owning pointer across a call arm.
//!
//! The mainline allocator keeps the selected arm's object in r5, allowing the
//! initial timer load in r4 to feed both the guard and decrement. Generic
//! lowering instead reloads the timer and delays the object copy until the
//! member call. This final-physical owner recognizes the complete packet before
//! removing those redundant operations.

use super::*;

impl Generator {
    pub(crate) fn schedule_guarded_member_decrement_arm_pointer(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        let Some(start) = recognize(&self.output.instructions) else {
            return;
        };

        crate::insert_instruction_retargeting(
            self,
            start + 3,
            Instruction::move_register(5, Eabi::FIRST_GENERAL_ARGUMENT),
        );
        let Instruction::LoadHalfwordAlgebraic { d, .. } =
            &mut self.output.instructions[start + 4]
        else {
            unreachable!("guarded timer load changed after recognition")
        };
        *d = 4;
        let Instruction::CompareWordImmediate { a, .. } =
            &mut self.output.instructions[start + 5]
        else {
            unreachable!("guarded timer comparison changed after recognition")
        };
        *a = 4;

        crate::remove_instruction_retargeting_to_next(self, start + 7);
        for instruction in &mut self.output.instructions[start + 8..start + 15] {
            match instruction {
                Instruction::StoreHalfword { a, .. }
                | Instruction::LoadFloatSingle { a, .. }
                | Instruction::StoreFloatSingle { a, .. } => *a = 5,
                _ => unreachable!("guarded member transfer changed after recognition"),
            }
        }
        crate::remove_instruction_retargeting_to_next(self, start + 15);

        // Both source arms join at the old LR reload. MWCC materializes the
        // constant result immediately before it, so keep the label on this
        // position while exchanging the two independent instructions.
        self.output.instructions.swap(start + 21, start + 22);
    }
}

fn recognize(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(27).position(|window| {
        matches!(window, [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::LoadHalfwordAlgebraic { d: 0, a: 3, offset: guard_offset },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { target: false_arm, .. },
            Instruction::LoadHalfwordAlgebraic { d: 4, a: 3, offset: reload_offset },
            Instruction::AddImmediate { d: 0, a: 4, immediate: -1 },
            Instruction::StoreHalfword { s: 0, a: 3, offset: store_offset },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: first_source },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: first_target },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: second_source },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: second_target },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: third_source },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: third_target },
            Instruction::Or { a: 5, s: 3, b: 3 },
            Instruction::AddImmediate { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 5, immediate: member_address },
            Instruction::BranchAndLink { .. },
            Instruction::Branch { target: join },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 1 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ] if guard_offset == reload_offset
            && guard_offset == store_offset
            && *second_source == first_source.saturating_add(4)
            && *third_source == second_source.saturating_add(4)
            && *second_target == first_target.saturating_add(4)
            && *third_target == second_target.saturating_add(4)
            && *member_address == *first_target
            && *false_arm == 21
            && *join == 22)
    })
}
