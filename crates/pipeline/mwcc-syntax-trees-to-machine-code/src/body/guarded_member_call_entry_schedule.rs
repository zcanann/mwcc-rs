//! Entry scheduling for a nullable member used as a direct-call receiver.
//!
//! A guard-return function can test `owner->member` and then pass both that
//! member and the owner to a call. Build 163 protects the owner in r4 in the
//! first linkage latency slot, loads the member directly into r3, and reuses it
//! across the null branch. Generic source-order lowering initially tests in r0
//! and rematerializes both arguments only on the taken edge; this physical pass
//! joins those independently correct pieces after allocation.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GuardedMemberCallEntry {
    start: usize,
    condition_load: usize,
    condition_compare: usize,
    owner_copy: usize,
    receiver_reload: usize,
}

impl Generator {
    pub(crate) fn schedule_guarded_member_call_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(plan) = recognize(&self.output.instructions) else {
            return;
        };

        match &mut self.output.instructions[plan.condition_load] {
            Instruction::LoadWord { d, .. } => *d = Eabi::FIRST_GENERAL_ARGUMENT,
            _ => unreachable!("the guarded member condition load was recognized"),
        }
        match &mut self.output.instructions[plan.condition_compare] {
            Instruction::CompareLogicalWordImmediate { a, .. } => {
                *a = Eabi::FIRST_GENERAL_ARGUMENT
            }
            _ => unreachable!("the guarded member condition comparison was recognized"),
        }

        crate::remove_instruction_retargeting_to_next(self, plan.receiver_reload);
        crate::move_instruction_before_retargeting(self, plan.owner_copy, plan.start + 1);
        // The taken edge originally entered at the owner copy. That copy is
        // now unconditional in the prologue, so the edge must enter at the
        // call rather than follow the moved instruction back to the entry.
        match &mut self.output.instructions[plan.start + 6] {
            Instruction::BranchConditionalForward { target, .. } => *target = plan.start + 9,
            _ => unreachable!("the guarded member taken edge was recognized"),
        }
    }
}

fn recognize(instructions: &[Instruction]) -> Option<GuardedMemberCallEntry> {
    instructions.windows(11).enumerate().find_map(|(start, window)| {
        let [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, .. },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
            Instruction::LoadWord { d: 0, a: 3, offset: condition_offset },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { target: call_edge, .. },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::Branch { target: join },
            Instruction::Or { a: 4, s: 3, b: 3 },
            Instruction::LoadWord { d: 3, a: 3, offset: receiver_offset },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        (*condition_offset == *receiver_offset
            && *call_edge == start + 8
            && *join > start + 10)
            .then_some(GuardedMemberCallEntry {
                start,
                condition_load: start + 3,
                condition_compare: start + 4,
                owner_copy: start + 8,
                receiver_reload: start + 9,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_nullable_member_receiver_and_owner_pair() {
        let instructions = [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::LoadWord { d: 0, a: 3, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 8 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::Branch { target: 15 },
            Instruction::Or { a: 4, s: 3, b: 3 },
            Instruction::LoadWord { d: 3, a: 3, offset: 12 },
            Instruction::BranchAndLink { target: "updatable".into() },
        ];
        assert_eq!(
            recognize(&instructions),
            Some(GuardedMemberCallEntry {
                start: 0,
                condition_load: 3,
                condition_compare: 4,
                owner_copy: 8,
                receiver_reload: 9,
            }),
        );
    }

    #[test]
    fn rejects_a_different_call_receiver_member() {
        let mut instructions = [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::LoadWord { d: 0, a: 3, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 8 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::Branch { target: 15 },
            Instruction::Or { a: 4, s: 3, b: 3 },
            Instruction::LoadWord { d: 3, a: 3, offset: 12 },
            Instruction::BranchAndLink { target: "updatable".into() },
        ];
        instructions[9] = Instruction::LoadWord { d: 3, a: 3, offset: 16 };
        assert_eq!(recognize(&instructions), None);
    }
}
