//! Physical value register for a coalesced loop-exit poll ladder.
//!
//! When MWCC reuses a loop-carried parameter home for the result selected by
//! that loop's exits, it keeps the polled member in volatile `r0`. Selection
//! and home coloring happen before physical allocation, so this late pass uses
//! explicit lifetime provenance and validates the complete comparison ladder.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_loop_exit_poll_register(&mut self) {
        if !self.structured_loop_exit_parameter_home_reuse {
            return;
        }
        let Some(plan) = loop_exit_poll_register_schedule(&self.output.instructions) else {
            return;
        };

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.load] else {
            unreachable!("validated loop-exit poll load changed form")
        };
        *d = 0;
        for compare in plan.compares {
            let Instruction::CompareWordImmediate { a, .. } =
                &mut self.output.instructions[compare]
            else {
                unreachable!("validated loop-exit poll comparison changed form")
            };
            *a = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LoopExitPollRegisterSchedule {
    load: usize,
    compares: [usize; 3],
}

fn loop_exit_poll_register_schedule(
    instructions: &[Instruction],
) -> Option<LoopExitPollRegisterSchedule> {
    (0..instructions.len().saturating_sub(12)).find_map(|load| {
        let packet = &instructions[load..load + 13];
        matches!(
            packet,
            [
                Instruction::LoadWord { d: 3, a: 30, offset: 12 },
                Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 30, a: 0, immediate: 0 }
                    | Instruction::LoadWord { d: 30, a: 30, offset: 32 },
                Instruction::Branch { .. },
                Instruction::CompareWordImmediate { a: 3, immediate: -1 },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 30, a: 0, immediate: -1 },
                Instruction::Branch { .. },
                Instruction::CompareWordImmediate { a: 3, immediate: 10 },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 30, a: 0, immediate: -3 },
                Instruction::Branch { .. },
            ]
        )
        .then_some(LoopExitPollRegisterSchedule {
            load,
            compares: [load + 1, load + 5, load + 9],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_three_way_loop_exit_poll_ladder() {
        let mut instructions = vec![
            Instruction::LoadWord { d: 3, a: 30, offset: 12 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 5 },
            Instruction::load_immediate(30, 0),
            Instruction::Branch { target: 13 },
            Instruction::CompareWordImmediate { a: 3, immediate: -1 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 9 },
            Instruction::load_immediate(30, -1),
            Instruction::Branch { target: 13 },
            Instruction::CompareWordImmediate { a: 3, immediate: 10 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 13 },
            Instruction::load_immediate(30, -3),
            Instruction::Branch { target: 13 },
        ];

        assert_eq!(
            loop_exit_poll_register_schedule(&instructions),
            Some(LoopExitPollRegisterSchedule {
                load: 0,
                compares: [1, 5, 9],
            })
        );
        instructions[3] = Instruction::LoadWord { d: 30, a: 30, offset: 32 };
        assert!(loop_exit_poll_register_schedule(&instructions).is_some());
    }

    #[test]
    fn rejects_a_different_exit_value_ladder() {
        let mut instructions = vec![
            Instruction::LoadWord { d: 3, a: 30, offset: 12 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 5 },
            Instruction::load_immediate(30, 0),
            Instruction::Branch { target: 13 },
            Instruction::CompareWordImmediate { a: 3, immediate: -1 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 9 },
            Instruction::load_immediate(30, -1),
            Instruction::Branch { target: 13 },
            Instruction::CompareWordImmediate { a: 3, immediate: 10 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 13 },
            Instruction::load_immediate(30, -2),
            Instruction::Branch { target: 13 },
        ];

        assert_eq!(loop_exit_poll_register_schedule(&instructions), None);
        instructions[11] = Instruction::load_immediate(30, -3);
        assert!(loop_exit_poll_register_schedule(&instructions).is_some());
    }
}
