//! Preserve a guarded member pointer for its fallthrough body.
//!
//! After `this` has moved to a saved home, a member-equality guard can load the
//! body pointer directly into the former receiver register. MWCC compares it
//! against the other member in r0 and reuses it after the guard instead of
//! loading the same pointer again.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_structured_guarded_member_pointer(&mut self) -> bool {
        let Some(plan) = plan(&self.output.instructions) else {
            return false;
        };
        let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[plan.first] else {
            unreachable!("guarded member load changed after recognition")
        };
        *d = plan.scratch;
        *a = plan.reused;
        let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[plan.second] else {
            unreachable!("guarded body pointer changed after recognition")
        };
        *d = plan.reused;
        *a = plan.reused;
        self.output.instructions[plan.compare] = Instruction::CompareLogicalWord {
            a: plan.scratch,
            b: plan.reused,
        };
        crate::remove_instruction_retargeting_to_next(self, plan.reload);
        self.output.instructions[plan.receiver] = Instruction::move_register(plan.owner, plan.reused);
        crate::move_instruction_before_retargeting(self, plan.receiver, plan.receiver - 1);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    receiver: usize,
    owner: u8,
    first: usize,
    second: usize,
    compare: usize,
    reload: usize,
    scratch: u8,
    reused: u8,
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        if start == 0
            || !matches!(
                instructions[start - 1],
                Instruction::LoadFloatSingle { a: 0, offset: 0, .. }
            )
        {
            return None;
        }
        let receiver = match window[0] {
            Instruction::AddImmediate { d, a, immediate: 0 } => (d, a),
            Instruction::Or { a, s, b } if s == b => (a, s),
            _ => return None,
        };
        let [
            Instruction::LoadWord { d: first, a: first_owner, offset: first_offset },
            Instruction::LoadWord { d: second, a: second_owner, offset: second_offset },
            Instruction::CompareLogicalWord { a, b },
            Instruction::BranchConditionalForward { target, .. },
            Instruction::LoadWord { d: reused, a: reload_owner, offset: reload_offset },
        ] = &window[1..]
        else {
            return None;
        };
        (*first_owner == receiver.0
            && *second_owner == receiver.0
            && *reload_owner == receiver.0
            && first_offset != second_offset
            && second_offset == reload_offset
            && *a == *first
            && *b == *second
            && *second == 0
            && *reused == receiver.1
            && *target > start + 5)
            .then_some(Plan {
                receiver: start,
                owner: receiver.0,
                first: start + 1,
                second: start + 2,
                compare: start + 3,
                reload: start + 5,
                scratch: *second,
                reused: *reused,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_pointer_reloaded_after_a_member_guard() {
        let instructions = vec![
            Instruction::LoadFloatSingle { d: 40, a: 0, offset: 0 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 40, a: 30, offset: 252 },
            Instruction::LoadWord { d: 0, a: 30, offset: 4 },
            Instruction::CompareLogicalWord { a: 40, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 12 },
            Instruction::LoadWord { d: 3, a: 30, offset: 4 },
        ];
        assert_eq!(
            plan(&instructions),
            Some(Plan {
                receiver: 1,
                owner: 30,
                first: 2,
                second: 3,
                compare: 4,
                reload: 6,
                scratch: 0,
                reused: 3,
            })
        );
    }
}
