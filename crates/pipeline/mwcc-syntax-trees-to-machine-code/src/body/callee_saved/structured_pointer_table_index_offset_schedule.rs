//! Final issue order for a retained repeated pointer-table byte offset.

use super::*;

impl Generator {
    pub(crate) fn schedule_prescaled_pointer_table_index(&mut self) {
        if !self.structured_prescaled_pointer_table_index {
            return;
        }
        self.schedule_prescaled_pointer_table_parameter_copies();
        self.schedule_prescaled_pointer_table_first_lookup();
        self.schedule_prescaled_pointer_table_following_lookup();
        self.schedule_prescaled_pointer_table_final_call();
    }

    fn schedule_prescaled_pointer_table_parameter_copies(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 29, .. },
                    Instruction::AddImmediate { d: 28, .. },
                    Instruction::AddImmediate { d: 27, .. },
                    Instruction::AddImmediate { d: 26, .. },
                    Instruction::AddImmediate { d: 25, .. },
                ]
            )
        }) else {
            return;
        };
        for destination in 25..=28 {
            let from = self.output.instructions[start..start + 5]
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::AddImmediate { d, .. } if *d == destination)
                })
                .map(|offset| start + offset)
                .expect("the descending parameter-copy packet was recognized");
            crate::move_instruction_before_retargeting(
                self,
                from,
                start + usize::from(destination - 25),
            );
        }
    }

    fn schedule_prescaled_pointer_table_first_lookup(&mut self) {
        let Some(start) = self.output.instructions.windows(7).position(|window| {
            matches!(
                window,
                [
                    Instruction::BranchAndLink { .. },
                    Instruction::ShiftLeftImmediate { a: 31, s: 30, shift: 2 },
                    Instruction::LoadWord { d: 0, .. },
                    Instruction::Add { d: 30, a: 0, b: 26 },
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                    Instruction::LoadWordIndexed { d: 4, a: 4, b: 31 },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 5, start + 1);
        crate::move_instruction_before_retargeting(self, start + 5, start + 4);
        crate::move_instruction_before_retargeting(self, start + 6, start + 5);
    }

    fn schedule_prescaled_pointer_table_following_lookup(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                    Instruction::LoadWordIndexed { d: 4, a: 4, b: 31 },
                    Instruction::AddImmediate { d: 4, a: 4, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 1, start);
    }

    fn schedule_prescaled_pointer_table_final_call(&mut self) {
        let Some(start) = self.output.instructions.windows(8).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
                    Instruction::AddImmediate { d: 5, a: 25, immediate: 0 },
                    Instruction::AddImmediate { d: 4, a: 1, .. },
                    Instruction::AddImmediate { d: 6, a: 30, immediate: 0 },
                    Instruction::AddImmediate { d: 7, a: 27, immediate: 0 },
                    Instruction::AddImmediate { d: 8, a: 0, immediate: 0 },
                    Instruction::Or { a: 9, s: 28, b: 28 },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
        crate::move_instruction_before_retargeting(self, start + 6, start + 4);
        self.output.instructions[start + 4] = Instruction::AddImmediate {
            d: 9,
            a: 28,
            immediate: 0,
        };
    }
}

