//! Nullable member-chain caching in a guarded callback transaction.
//!
//! The relationship guard leaves a channel member in `r3` on its only
//! continuing edge.  A later nullable data-chain test can therefore use `r5`
//! throughout and call the channel operation with the still-live `r3`, rather
//! than reloading the channel immediately before the call.

use super::*;

impl Generator {
    pub(crate) fn schedule_guarded_callback_nullable_member_chain(
        &mut self,
        function: &Function,
    ) {
        let Some(plan) = super::structured_guarded_member_lvalue::recognize(function) else {
            return;
        };
        let Some(receiver) = super::structured_guarded_callback_copy_schedule::retained_receiver(
            &self.output.instructions,
            plan.member_offset,
        ) else {
            return;
        };
        let Some(start) = nullable_member_chain(&self.output.instructions, receiver) else {
            return;
        };
        let cached_channel = self.output.instructions[..start].windows(3).any(|window| {
            matches!(window, [
                Instruction::LoadWord { d: 3, a, offset: 32 },
                Instruction::CompareLogicalWord { a: 3, .. },
                Instruction::BranchConditionalForward { .. },
            ] if *a == receiver)
        });
        if !cached_channel {
            return;
        }

        self.output.instructions[start] = Instruction::LoadWord {
            d: 5,
            a: receiver,
            offset: 16,
        };
        self.output.instructions[start + 1] = Instruction::CompareLogicalWordImmediate {
            a: 5,
            immediate: 0,
        };
        self.output.instructions[start + 3] = Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: 36,
        };
        self.output.instructions[start + 4] = Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        };
        crate::remove_instruction_retargeting_to_next(self, start + 7);
    }
}

fn nullable_member_chain(instructions: &[Instruction], receiver: u8) -> Option<usize> {
    instructions.windows(9).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 0, a, offset: 16 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 3, a: 0, offset: 36 },
            Instruction::LoadWord { d: 0, a: 3, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 3, a: call_base, offset: 32 },
            Instruction::BranchAndLink { .. },
        ] if *a == receiver && *call_base == receiver)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_nullable_chain_with_reloaded_call_receiver() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 30, offset: 16 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 9 },
            Instruction::LoadWord { d: 3, a: 0, offset: 36 },
            Instruction::LoadWord { d: 0, a: 3, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 9 },
            Instruction::LoadWord { d: 3, a: 30, offset: 32 },
            Instruction::BranchAndLink { target: "stop".into() },
        ];
        assert_eq!(nullable_member_chain(&instructions, 30), Some(0));
    }
}
