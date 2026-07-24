//! Load scheduling inside integer sign-clamp diamonds.
//!
//! A negative arm that combines a member load with the signed guard has two
//! independent register operations before that load. MWCC issues the load as
//! soon as the branch falls through, then fills its latency with the negate and
//! zero. Keep this separate from the wider packet-loop lane assignment.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_frame_sign_clamp_load(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(10)
            .enumerate()
            .find_map(|(start, window)| is_member_sign_clamp(window, start).then_some(start))
        else {
            return;
        };

        self.move_instruction_before(start + 4, start + 2);
    }
}

fn is_member_sign_clamp(window: &[Instruction], start: usize) -> bool {
    let [Instruction::CompareWordImmediate {
        a: guard,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: else_start, ..
    }, Instruction::Negate {
        d: negative,
        a: negate_source,
    }, Instruction::AddImmediate {
        d: positive,
        a: 0,
        immediate: 0,
    }, Instruction::LoadHalfwordZero {
        d: loaded,
        a: base,
        offset,
    }, Instruction::Add {
        d: negative_result,
        a: add_a,
        b: add_b,
    }, Instruction::Branch { target: join }, Instruction::LoadHalfwordZero {
        d: positive_result,
        a: positive_base,
        offset: positive_offset,
    }, Instruction::Or {
        a: positive_copy,
        s: copy_source,
        b: copy_source_again,
    }, Instruction::AddImmediate {
        d: negative_zero,
        a: 0,
        immediate: 0,
    }] = window
    else {
        return false;
    };

    *else_start == start + 7
        && *join == start + 10
        && guard == negate_source
        && negative == negative_zero
        && positive == positive_copy
        && copy_source == guard
        && copy_source_again == guard
        && negative_result == positive_result
        && base == positive_base
        && offset == positive_offset
        && ((*add_a == *loaded && *add_b == *guard) || (*add_b == *loaded && *add_a == *guard))
        && loaded != guard
        && loaded != negative
        && loaded != positive
        && base != guard
        && base != negative
        && base != positive
}

#[cfg(test)]
#[path = "structured_frame_clamp_schedule_tests.rs"]
mod tests;
