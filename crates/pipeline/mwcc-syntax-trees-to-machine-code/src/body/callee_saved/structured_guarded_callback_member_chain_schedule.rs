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
        self.schedule_guarded_callback_priority_update(receiver);
    }

    fn schedule_guarded_callback_priority_update(&mut self, receiver: u8) {
        let Some(start) = priority_update(&self.output.instructions, receiver) else {
            return;
        };
        self.output.instructions[start + 1] = Instruction::CompareLogicalWordImmediate {
            a: 3,
            immediate: 0,
        };
        self.output.instructions[start + 2] = Instruction::RotateAndMask {
            a: 4,
            s: 0,
            shift: 16,
            begin: 24,
            end: 31,
        };
        self.output.instructions[start + 5] = Instruction::LoadByteZeroWithUpdate {
            d: 0,
            a: 3,
            offset: 3,
        };
        self.output.instructions[start + 10] = Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: 0,
        };
        for removed in [start + 9, start + 6, start + 3] {
            crate::remove_instruction_retargeting_to_next(self, removed);
        }
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

fn priority_update(instructions: &[Instruction], receiver: u8) -> Option<usize> {
    instructions.windows(11).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 0, a, offset: 288 },
            Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 16 },
            Instruction::LoadWord { d: 0, a: channel_base, offset: 32 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadByteZero { d: 0, a: 0, offset: 3 },
            Instruction::ClearLeftImmediate { a: 4, s: 3, clear: 24 },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 4, a: store_base, offset: 32 },
            Instruction::StoreByte { s: 3, a: 4, offset: 3 },
        ] if *a == receiver && *channel_base == receiver && *store_base == receiver)
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

    #[test]
    fn recognizes_priority_update_with_reloaded_channel() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 30, offset: 288 },
            Instruction::ShiftRightLogicalImmediate { a: 3, s: 0, shift: 16 },
            Instruction::LoadWord { d: 0, a: 30, offset: 32 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 11 },
            Instruction::LoadByteZero { d: 0, a: 0, offset: 3 },
            Instruction::ClearLeftImmediate { a: 4, s: 3, clear: 24 },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 11 },
            Instruction::LoadWord { d: 4, a: 30, offset: 32 },
            Instruction::StoreByte { s: 3, a: 4, offset: 3 },
        ];
        assert_eq!(priority_update(&instructions, 30), Some(0));
    }
}
