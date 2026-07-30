//! Latency schedule for a converted store followed by a four-word call.
//!
//! Build 163 fills the floating conversion/store latency with the following
//! call's independent argument setup.  Selection keeps the two operations
//! contiguous; this physical pass interleaves their dependency-complete window.

#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 9] = [0, 4, 5, 6, 1, 7, 2, 3, 8];

impl Generator {
    pub(crate) fn schedule_structured_conversion_following_call(&mut self) -> bool {
        let Some(start) = self
            .output
            .instructions
            .windows(SCHEDULE.len())
            .position(conversion_following_call)
        else {
            return false;
        };
        let Instruction::Or {
            a: destination,
            s: source,
            b,
        } = self.output.instructions[start + 4]
        else {
            unreachable!("conversion-call argument move changed after recognition")
        };
        debug_assert_eq!(source, b);
        self.output.instructions[start + 4] = Instruction::AddImmediate {
            d: destination,
            a: source,
            immediate: 0,
        };
        permute_region(&mut self.output, start, &SCHEDULE);
        true
    }
}

fn conversion_following_call(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::ConvertToIntegerWordZero { d: converted, .. },
            Instruction::StoreFloatDouble {
                s: stored,
                a: 1,
                offset: conversion_offset,
            },
            Instruction::LoadWord {
                d: loaded,
                a: 1,
                offset: word_offset,
            },
            Instruction::StoreWord { s: value, .. },
            Instruction::Or {
                a: 3,
                s: receiver,
                b: receiver_again,
            },
            Instruction::AddImmediate { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::AddImmediate { d: 6, a: 0, .. },
            Instruction::BranchAndLink { .. },
        ] if *converted == *stored
            && *loaded == *value
            && *word_offset == *conversion_offset + 4
            && *receiver == *receiver_again
    )
}

fn permute_region(
    output: &mut mwcc_machine_code::MachineFunction,
    start: usize,
    schedule: &[usize],
) {
    let original = output.instructions[start..start + schedule.len()].to_vec();
    for (destination, &source) in schedule.iter().enumerate() {
        output.instructions[start + destination] = original[source].clone();
    }
    let mut inverse = vec![0usize; schedule.len()];
    for (new_index, &old_index) in schedule.iter().enumerate() {
        inverse[old_index] = new_index;
    }
    for relocation in &mut output.relocations {
        if (start..start + schedule.len()).contains(&relocation.instruction_index) {
            relocation.instruction_index = start + inverse[relocation.instruction_index - start];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_conversion_store_before_four_call_arguments() {
        let window = vec![
            Instruction::ConvertToIntegerWordZero { d: 0, b: 1 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 56,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 60,
            },
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 8,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 2,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: 3,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(conversion_following_call(&window));
    }
}
