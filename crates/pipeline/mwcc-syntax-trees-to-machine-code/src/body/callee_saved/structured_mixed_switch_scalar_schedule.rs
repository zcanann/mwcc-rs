//! Physical schedule for mixed-width scalar frames feeding multiple switches.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_mixed_switch_scalar_frame(&mut self) {
        if !self.structured_packed_switch_scalar_frame {
            return;
        }
        let Some(owner) = self.output.instructions.iter().find_map(|instruction| match instruction {
            Instruction::Or { a, s: 3, b: 3 } if *a != 3 => Some(*a),
            _ => None,
        }) else {
            return;
        };
        let range_start = self.output.instructions.windows(4).position(|window| {
            matches!(window[0], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
                && matches!(window[1], Instruction::Or { a: 4, s: 0, b: 0 })
                && matches!(window[2], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
                && matches!(window[3], Instruction::CompareWord { a: 4, b: 0 })
        });
        let Some(range_start) = range_start else {
            return;
        };
        let mask_count = self.output.instructions.iter().filter(|instruction| {
            matches!(instruction, Instruction::AndContiguousMask {
                a: 3,
                s: 0,
                begin: 29,
                end: 31,
            })
        }).count();
        let access_call_count = self.output.instructions.windows(6).filter(|window| {
            matches!(window[0], Instruction::LoadHalfwordZero { d: 3, a: 1, .. })
                && matches!(window[1], Instruction::LoadHalfwordZero { d: 4, a: 1, .. })
                && matches!(window[2], Instruction::Or { a: 5, s, b }
                    if s == owner && b == owner)
                && matches!(window[3], Instruction::AddImmediate { d: 6, a: 1, .. })
                && matches!(window[4], Instruction::AddImmediate { d: 7, a: 0, immediate: 1 })
                && matches!(window[5], Instruction::BranchAndLink { .. })
        }).count();
        if mask_count != 1 || access_call_count != 4 {
            return;
        }

        self.output.instructions[range_start] = match self.output.instructions[range_start] {
            Instruction::LoadHalfwordZero { a, offset, .. } => {
                Instruction::LoadHalfwordZero { d: 4, a, offset }
            }
            _ => unreachable!("the mixed-width range load was matched"),
        };
        crate::remove_instruction_retargeting_to_next(self, range_start + 1);
        self.output.instructions[range_start + 2] = Instruction::CompareLogicalWord { a: 4, b: 0 };

        let mask = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::AndContiguousMask {
                a: 3,
                s: 0,
                begin: 29,
                end: 31,
            })
        }).expect("the mixed-width switch mask was prevalidated");
        self.output.instructions[mask] = Instruction::AndContiguousMask {
            a: 0,
            s: 0,
            begin: 29,
            end: 31,
        };
        for instruction in &mut self.output.instructions[mask + 1..] {
            match instruction {
                Instruction::CompareWordImmediate { a: 3, immediate }
                    if (0..=4).contains(immediate) =>
                {
                    *instruction = Instruction::CompareWordImmediate {
                        a: 0,
                        immediate: *immediate,
                    };
                }
                Instruction::LoadHalfwordZero { d: 3, a: 1, .. } => break,
                _ => {}
            }
        }

        let access_calls = self.output.instructions.windows(6).enumerate().filter_map(|(start, window)| {
            (matches!(window[0], Instruction::LoadHalfwordZero { d: 3, a: 1, .. })
                && matches!(window[1], Instruction::LoadHalfwordZero { d: 4, a: 1, .. })
                && matches!(window[2], Instruction::Or { a: 5, s, b }
                    if s == owner && b == owner)
                && matches!(window[3], Instruction::AddImmediate { d: 6, a: 1, .. })
                && matches!(window[4], Instruction::AddImmediate { d: 7, a: 0, immediate: 1 })
                && matches!(window[5], Instruction::BranchAndLink { .. }))
            .then_some(start)
        }).collect::<Vec<_>>();
        for start in access_calls {
            self.output.instructions.swap(start + 1, start + 2);
        }

        let owner_copies = self.output.instructions.iter().enumerate().filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::Or { a: 3, s, b }
                if *s == owner && *b == owner)
            .then_some(index)
        }).collect::<Vec<_>>();
        for index in owner_copies.into_iter().rev().skip(1) {
            self.output.instructions[index] = Instruction::AddImmediate {
                d: 3,
                a: owner,
                immediate: 0,
            };
        }
    }
}
