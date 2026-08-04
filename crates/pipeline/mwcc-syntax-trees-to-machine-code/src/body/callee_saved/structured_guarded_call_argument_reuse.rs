//! Retain a byte guard in the following call-argument register.
//!
//! When the nonzero arm passes the guarded byte to a call, MWCC loads it into
//! r5 for the guard itself. The zero arm skips the call, while the taken arm
//! reuses r5 and avoids a second member load.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_structured_guarded_call_argument(&mut self) -> bool {
        let Some(plan) = plan(&self.output.instructions) else {
            return false;
        };
        let Instruction::LoadByteZero { d, .. } = &mut self.output.instructions[plan.guard] else {
            unreachable!("guard byte changed after recognition")
        };
        *d = plan.argument;
        let Instruction::CompareLogicalWordImmediate { a, .. } =
            &mut self.output.instructions[plan.guard + 1]
        else {
            unreachable!("guard comparison changed after recognition")
        };
        *a = plan.argument;
        crate::remove_instruction_retargeting_to_next(self, plan.reload);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    guard: usize,
    reload: usize,
    argument: u8,
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    instructions.windows(9).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadByteZero { d: guard, a: owner, offset },
            Instruction::CompareLogicalWordImmediate { a: compared, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { .. },
            Instruction::Branch { .. },
            Instruction::AddImmediate { d: 3, .. },
            Instruction::AddImmediate { d: 4, .. },
            Instruction::LoadByteZero { d: argument, a: reload_owner, offset: reload_offset },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        (*guard == 0
            && *compared == *guard
            && *argument == 5
            && *reload_owner == *owner
            && *reload_offset == *offset)
            .then_some(Plan {
                guard: start,
                reload: start + 7,
                argument: *argument,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_guard_byte_reloaded_for_a_call() {
        let instructions = vec![
            Instruction::LoadByteZero { d: 0, a: 30, offset: 184 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 5 },
            Instruction::LoadFloatSingle { d: 31, a: 0, offset: 0 },
            Instruction::Branch { target: 9 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 200 },
            Instruction::AddImmediate { d: 4, a: 30, immediate: 188 },
            Instruction::LoadByteZero { d: 5, a: 30, offset: 184 },
            Instruction::BranchAndLink { target: "sample".into() },
        ];
        assert_eq!(
            plan(&instructions),
            Some(Plan { guard: 0, reload: 7, argument: 5 })
        );
    }
}
