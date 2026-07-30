//! Independent call arguments scheduled into load-latency slots.
//!
//! Selection emits argument expressions in source order. Build 163 instead
//! moves dependency-free argument materializations between a load and its
//! consumer. Keep these physical permutations together: each recognizer proves
//! the complete dependency window and preserves every relocation.

#[allow(unused_imports)]
use super::*;

use super::structured_conversion_call_schedule::permute_region;

const AGGREGATE_RECEIVER_SCHEDULE: [usize; 10] = [0, 1, 2, 8, 3, 4, 5, 6, 7, 9];
const FLOAT_ARGUMENT_SCHEDULE: [usize; 4] = [2, 0, 1, 3];
const MEMBER_ARGUMENT_SCHEDULE: [usize; 6] = [2, 0, 1, 3, 4, 5];

impl Generator {
    pub(crate) fn schedule_structured_argument_load_latency(&mut self) -> bool {
        let mut changed = false;

        while let Some(start) = self
            .output
            .instructions
            .windows(AGGREGATE_RECEIVER_SCHEDULE.len())
            .position(aggregate_copy_before_receiver)
        {
            permute_region(&mut self.output, start, &AGGREGATE_RECEIVER_SCHEDULE);
            changed = true;
        }

        while let Some(start) = self
            .output
            .instructions
            .windows(FLOAT_ARGUMENT_SCHEDULE.len())
            .position(float_load_after_arguments)
        {
            normalize_argument_copy(&mut self.output.instructions[start]);
            permute_region(&mut self.output, start, &FLOAT_ARGUMENT_SCHEDULE);
            changed = true;
        }

        while let Some(start) = self
            .output
            .instructions
            .windows(MEMBER_ARGUMENT_SCHEDULE.len())
            .position(member_chain_after_arguments)
        {
            normalize_argument_copy(&mut self.output.instructions[start]);
            normalize_argument_copy(&mut self.output.instructions[start + 1]);
            permute_region(&mut self.output, start, &MEMBER_ARGUMENT_SCHEDULE);
            changed = true;
        }

        changed
    }
}

fn aggregate_copy_before_receiver(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediateShifted { d: anchor, .. },
            Instruction::AddImmediate {
                d: aggregate,
                a: anchor_source,
                ..
            },
            Instruction::LoadWord {
                d: first,
                a: first_base,
                offset: first_offset,
            },
            Instruction::LoadWord {
                d: second,
                a: second_base,
                offset: second_offset,
            },
            Instruction::StoreWord {
                s: first_value,
                a: 1,
                offset: first_target,
            },
            Instruction::StoreWord {
                s: second_value,
                a: 1,
                offset: second_target,
            },
            Instruction::LoadWord {
                d: third,
                a: third_base,
                offset: third_offset,
            },
            Instruction::StoreWord {
                s: third_value,
                a: 1,
                offset: third_target,
            },
            Instruction::Or {
                a: 3,
                s: receiver,
                b: receiver_again,
            },
            Instruction::BranchAndLink { .. },
        ] if *anchor == *anchor_source
            && *aggregate == *first_base
            && *aggregate == *second_base
            && *aggregate == *third_base
            && *first == *first_value
            && *second == *second_value
            && *third == *third_value
            && *receiver == *receiver_again
            && *receiver >= 14
            && first_offset.checked_add(4) == Some(*second_offset)
            && second_offset.checked_add(4) == Some(*third_offset)
            && first_target.checked_add(4) == Some(*second_target)
            && second_target.checked_add(4) == Some(*third_target)
    )
}

fn float_load_after_arguments(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::Or {
                a: 3,
                s: receiver,
                b: receiver_again,
            },
            Instruction::AddImmediate { d: 4, a: 1, .. },
            Instruction::LoadFloatSingle {
                d: 1,
                a: float_base,
                ..
            },
            Instruction::BranchAndLink { .. },
        ] if *receiver == *receiver_again
            && *receiver >= 14
            && !matches!(*float_base, 3 | 4)
    )
}

fn member_chain_after_arguments(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::Or {
                a: 3,
                s: receiver,
                b: receiver_again,
            },
            Instruction::Or {
                a: 4,
                s: second_argument,
                b: second_argument_again,
            },
            Instruction::LoadWord {
                d: 5,
                a: member_base,
                ..
            },
            Instruction::LoadWord { d: 5, a: 5, .. },
            Instruction::LoadByteZero { d: 5, a: 5, .. },
            Instruction::BranchAndLink { .. },
        ] if *receiver == *receiver_again
            && *second_argument == *second_argument_again
            && *receiver >= 14
            && *second_argument >= 14
            && *member_base >= 14
    )
}

fn normalize_argument_copy(instruction: &mut Instruction) {
    let Instruction::Or {
        a: destination,
        s: source,
        b,
    } = *instruction
    else {
        unreachable!("argument copy changed after recognition")
    };
    debug_assert_eq!(source, b);
    *instruction = Instruction::AddImmediate {
        d: destination,
        a: source,
        immediate: 0,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_three_word_aggregate_copy_before_its_receiver() {
        let window = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 5,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 5,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 4,
                a: 1,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 48,
            },
            Instruction::LoadWord {
                d: 0,
                a: 5,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 52,
            },
            Instruction::Or { a: 3, s: 30, b: 30 },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(aggregate_copy_before_receiver(&window));
    }

    #[test]
    fn recognizes_a_relocatable_float_argument_after_integer_arguments() {
        let window = vec![
            Instruction::Or { a: 3, s: 30, b: 30 },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 44,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(float_load_after_arguments(&window));
    }

    #[test]
    fn recognizes_a_member_chain_after_two_forwarded_arguments() {
        let window = vec![
            Instruction::Or { a: 3, s: 30, b: 30 },
            Instruction::Or { a: 4, s: 29, b: 29 },
            Instruction::LoadWord {
                d: 5,
                a: 31,
                offset: 268,
            },
            Instruction::LoadWord {
                d: 5,
                a: 5,
                offset: 8,
            },
            Instruction::LoadByteZero {
                d: 5,
                a: 5,
                offset: 18,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(member_chain_after_arguments(&window));
    }
}
