//! Final physical schedule for guarded calls that fill packed scalar outputs.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_guarded_scalar_output_frame(&mut self) {
        if !self.structured_guarded_scalar_output_frame {
            return;
        }
        let Some(owner) = self.output.instructions.iter().find_map(|instruction| match instruction {
            Instruction::Or { a, s: 3, b: 3 } if *a != 3 => Some(*a),
            _ => None,
        }) else {
            return;
        };
        let result_home = self
            .output
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                Instruction::StoreWord { s, a: 1, .. }
                    if *s >= 14 && *s != owner => Some(*s),
                _ => None,
            });
        let Some(result_home) = result_home else {
            return;
        };

        let frame_output_calls = self
            .output
            .instructions
            .windows(3)
            .enumerate()
            .filter_map(|(start, window)| {
                (matches!(
                    window[0],
                    Instruction::Or { a: 3, s, b } if s == owner && b == owner
                ) && matches!(
                    window[1],
                    Instruction::AddImmediate { d: 4, a: 1, .. }
                ) && matches!(window[2], Instruction::BranchAndLink { .. }))
                .then_some(start + 2)
            })
            .collect::<Vec<_>>();
        if frame_output_calls.len() != 4 {
            return;
        }

        for (ordinal, call) in frame_output_calls.iter().copied().enumerate().rev() {
            if ordinal == 0 {
                self.output.instructions[call + 1] = Instruction::OrRecord {
                    a: result_home,
                    s: 3,
                    b: 3,
                };
            } else {
                crate::insert_instruction_retargeting(
                    self,
                    call + 1,
                    Instruction::Or {
                        a: result_home,
                        s: 3,
                        b: 3,
                    },
                );
                if ordinal < 3 {
                    let Instruction::CompareWordImmediate { a, immediate: 0 } =
                        &mut self.output.instructions[call + 2]
                    else {
                        unreachable!("the guarded output result comparison was matched")
                    };
                    *a = result_home;
                }
            }
        }

        let Some((range_start, start_offset, end_offset)) =
            self.output.instructions.windows(3).enumerate().find_map(|(start, window)| {
                match window {
                    [
                        Instruction::LoadWord { d: 0, a: 1, offset: start_offset },
                        Instruction::LoadWord { d: 0, a: 1, offset: end_offset },
                        Instruction::CompareLogicalWord { a: 0, b: 0 },
                    ] if start_offset != end_offset => Some((start, *start_offset, *end_offset)),
                    _ => None,
                }
            })
        else {
            return;
        };
        self.output.instructions[range_start] = Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: start_offset,
        };
        self.output.instructions[range_start + 1] = Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: end_offset,
        };
        self.output.instructions[range_start + 2] = Instruction::CompareLogicalWord { a: 4, b: 5 };

        let Some(flush_start) = self.output.instructions.windows(4).position(|window| {
            matches!(window[0], Instruction::LoadByteZero { d: 3, a: 1, .. })
                && matches!(window[1], Instruction::LoadWord { d: 4, a: 1, offset } if offset == start_offset)
                && matches!(window[2], Instruction::LoadWord { d: 5, a: 1, offset } if offset == end_offset)
                && matches!(window[3], Instruction::BranchAndLink { .. })
        }) else {
            return;
        };
        if let Some(Instruction::CompareWordImmediate { a, immediate: 0 }) = self
            .output
            .instructions
            .get_mut(range_start + 3..flush_start)
            .and_then(|instructions| {
                instructions.iter_mut().rev().find(|instruction| {
                    matches!(instruction, Instruction::CompareWordImmediate { a: 3, immediate: 0 })
                })
            })
        {
            *a = result_home;
        }
        crate::remove_instruction_retargeting_to_next(self, flush_start + 2);
        crate::remove_instruction_retargeting_to_next(self, flush_start + 1);
        let flush_call = flush_start + 1;
        crate::insert_instruction_retargeting(
            self,
            flush_call + 1,
            Instruction::Or {
                a: result_home,
                s: 3,
                b: 3,
            },
        );
        let Instruction::CompareWordImmediate { a, immediate: 0 } =
            &mut self.output.instructions[flush_call + 2]
        else {
            return;
        };
        *a = result_home;

        if let Some(reply_call) = self.output.instructions.windows(3).position(|window| {
            matches!(window[0], Instruction::AddImmediate { d: 4, a: 0, immediate: 128 })
                && matches!(window[1], Instruction::AddImmediate { d: 5, a: 0, immediate: 0 })
                && matches!(window[2], Instruction::BranchAndLink { .. })
        }).map(|start| start + 2)
        {
            if let Some(Instruction::CompareWordImmediate { a, immediate: 0 }) =
                self.output.instructions.get_mut(reply_call + 1)
            {
                *a = result_home;
            }
        }

        if let Some(switch_compare) = self.output.instructions.windows(4).position(|window| {
            matches!(window[0], Instruction::CompareWordImmediate { a, immediate: 0 }
                if a == result_home)
                && matches!(window[1], Instruction::BranchConditionalForward { .. })
                && matches!(window[2], Instruction::CompareWordImmediate { a: 3, .. })
                && matches!(window[3], Instruction::BranchConditionalForward { .. })
        }).map(|start| start + 2)
        {
            let Instruction::CompareWordImmediate { a, .. } =
                &mut self.output.instructions[switch_compare]
            else {
                unreachable!("the guarded output error switch was matched")
            };
            *a = result_home;
            let (options, condition_bit, default) = match self.output.instructions[switch_compare + 1] {
                Instruction::BranchConditionalForward { options, condition_bit, target } => {
                    (options, condition_bit, target)
                }
                _ => unreachable!("the guarded output switch branch was matched"),
            };
            crate::insert_instruction_retargeting(
                self,
                switch_compare + 2,
                Instruction::Branch { target: default },
            );
            self.output.instructions[switch_compare + 1] =
                Instruction::BranchConditionalForward {
                    options: options ^ 8,
                    condition_bit,
                    target: switch_compare + 3,
                };
        }

        let owner_copies = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(instruction, Instruction::Or { a: 3, s, b }
                    if *s == owner && *b == owner)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        for index in owner_copies.into_iter().rev().skip(1) {
            self.output.instructions[index] = Instruction::AddImmediate {
                d: 3,
                a: owner,
                immediate: 0,
            };
        }
    }
}
