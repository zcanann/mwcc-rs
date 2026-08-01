//! Final schedule for packed scalar outputs shared by sparse switch bodies.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_shared_switch_scalar_frame(&mut self) {
        if !self.structured_shared_switch_scalar_frame {
            return;
        }
        let Some(owner) = self.output.instructions.iter().find_map(|instruction| match instruction {
            Instruction::Or { a, s: 3, b: 3 } if *a != 3 => Some(*a),
            _ => None,
        }) else {
            return;
        };

        let range_body = self.output.instructions.windows(7).position(|window| {
            matches!(window[0], Instruction::LoadWord { d: 3, a: 1, .. })
                && matches!(window[1], Instruction::LoadWord { d: 4, a: 1, .. })
                && matches!(window[2], Instruction::LoadByteZero { d: 0, a: 1, .. })
                && matches!(window[3], Instruction::SubtractFromImmediate { d: 0, a: 0, immediate: 17 })
                && matches!(window[4], Instruction::CountLeadingZeros { a: 0, s: 0 })
                && matches!(window[5], Instruction::ShiftRightLogicalImmediate { a: 5, s: 0, shift: 5 })
                && matches!(window[6], Instruction::BranchAndLink { .. })
        });
        let Some(range_body) = range_body else {
            return;
        };
        let single_body = self.output.instructions.windows(6).position(|window| {
            matches!(window[0], Instruction::LoadByteZero { d: 3, a: 1, .. })
                && matches!(window[1], Instruction::LoadByteZero { d: 0, a: 1, .. })
                && matches!(window[2], Instruction::SubtractFromImmediate { d: 0, a: 0, immediate: 16 })
                && matches!(window[3], Instruction::CountLeadingZeros { a: 0, s: 0 })
                && matches!(window[4], Instruction::ShiftRightLogicalImmediate { a: 4, s: 0, shift: 5 })
                && matches!(window[5], Instruction::BranchAndLink { .. })
        });
        let Some(single_body) = single_body.filter(|single_body| *single_body < range_body) else {
            return;
        };
        crate::remove_instruction_retargeting_to_next(self, range_body + 2);
        let range = self.output.instructions[range_body..range_body + 6].to_vec();
        self.output.instructions[range_body..range_body + 6].clone_from_slice(&[
            range[2].clone(),
            range[0].clone(),
            range[3].clone(),
            range[1].clone(),
            range[4].clone(),
            range[5].clone(),
        ]);

        crate::remove_instruction_retargeting_to_next(self, single_body + 1);
        crate::move_instruction_before_retargeting(self, single_body + 1, single_body);
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == single_body + 1 =>
                {
                    *target = single_body;
                }
                _ => {}
            }
        }

        for instruction in &mut self.output.instructions {
            if matches!(instruction, Instruction::Or { a: 3, s, b }
                if *s == owner && *b == owner)
            {
                *instruction = Instruction::AddImmediate {
                    d: 3,
                    a: owner,
                    immediate: 0,
                };
            }
        }
        if let Some(compare) = self.output.instructions.windows(2).position(|window| {
            matches!(window[0], Instruction::LoadByteZero { d: 0, a: 1, .. })
                && matches!(window[1], Instruction::CompareWordImmediate { a: 0, immediate: 1 })
        }).map(|load| load + 1)
        {
            self.output.instructions[compare] = Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 1,
            };
        }
    }
}
