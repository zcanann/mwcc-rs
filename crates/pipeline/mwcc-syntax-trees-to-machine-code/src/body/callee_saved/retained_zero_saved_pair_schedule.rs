//! Compact saved-pair framing for a receiver and a retained zero.
//!
//! When a two-home cleanup transaction keeps its receiver in r30 and a shared
//! zero in r31, MWCC saves and restores that contiguous suffix with `stmw/lmw`.
//! The general structured prologue emits dependency-safe scalar stores first;
//! this final owner fuses only the complete measured retained-zero frame.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedZeroSavedPair {
    first_store: usize,
    first_load: usize,
    frame_size: i16,
}

impl Generator {
    pub(crate) fn fuse_retained_zero_saved_pair(&mut self) {
        if !self.behavior.use_lmw_stmw {
            return;
        }
        let Some(plan) = retained_zero_saved_pair(&self.output.instructions) else {
            return;
        };

        self.output.instructions[plan.first_load] = Instruction::LoadMultipleWord {
            d: 30,
            a: 1,
            offset: plan.frame_size - 8,
        };
        crate::remove_instruction_retargeting_to_next(self, plan.first_load + 1);

        self.output.instructions[plan.first_store] = Instruction::StoreMultipleWord {
            s: 30,
            a: 1,
            offset: plan.frame_size - 8,
        };
        crate::remove_instruction_retargeting_to_next(self, plan.first_store + 1);
    }
}

fn retained_zero_saved_pair(instructions: &[Instruction]) -> Option<RetainedZeroSavedPair> {
    let first_store = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: 31, a: 1, .. },
                Instruction::StoreWord { s: 30, a: 1, .. },
                Instruction::Or { a: 30, s: 3, b: 3 },
            ]
        )
    })? + 3;
    let frame_size = match instructions[first_store - 1] {
        Instruction::StoreWordWithUpdate { offset, .. } if offset < 0 => -offset,
        _ => return None,
    };
    if !matches!(
        instructions[first_store..first_store + 2],
        [
            Instruction::StoreWord { s: 31, a: 1, offset: high },
            Instruction::StoreWord { s: 30, a: 1, offset: low },
        ] if high == frame_size - 4 && low == frame_size - 8
    ) {
        return None;
    }

    let first_call = instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { .. })
    })?;
    if first_call <= first_store
        || !instructions[first_store + 2..first_call]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::AddImmediate { d: 31, a: 0, immediate: 0 }))
        || !instructions[first_call..]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::StoreWord { s: 31, a: 30, .. }))
    {
        return None;
    }

    let first_load = instructions.windows(3).rposition(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord { d: 0, a: 1, offset: link },
                Instruction::LoadWord { d: 31, a: 1, offset: high },
                Instruction::LoadWord { d: 30, a: 1, offset: low },
            ] if *link == frame_size + 4
                && *high == frame_size - 4
                && *low == frame_size - 8
        )
    })? + 1;

    Some(RetainedZeroSavedPair {
        first_store,
        first_load,
        frame_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_receiver_and_retained_zero_saved_pair() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::move_register(30, 3),
            Instruction::load_immediate(31, 0),
            Instruction::BranchAndLink { target: "stop".into() },
            Instruction::StoreWord { s: 31, a: 30, offset: 32 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(
            retained_zero_saved_pair(&instructions),
            Some(RetainedZeroSavedPair {
                first_store: 3,
                first_load: 10,
                frame_size: 24,
            })
        );
    }
}
