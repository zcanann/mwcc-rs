//! Final schedule for a packed write-direction register transaction.
//!
//! The read sibling reloads its register range in each switch arm. For writes,
//! MWCC keeps the validated range in r3/r4, retains each access result in r31,
//! and emits the later sparse error mapping as a balanced comparison tree.
//! Both source frame classification and the complete physical topology gate the
//! rewrite; a miss restores the generator unchanged.

use super::*;
use super::structured_memory_transfer_schedule::canonicalize_owner_copies;

impl Generator {
    pub(crate) fn finalize_structured_write_register_frame(&mut self) {
        if !self.structured_packed_switch_scalar_frame {
            return;
        }
        let original = self.clone();
        if !self.try_finalize_structured_write_register_frame() {
            *self = original;
        }
    }

    fn try_finalize_structured_write_register_frame(&mut self) -> bool {
        let owner = 30;
        let result = 31;
        if !schedule_range(self)
            || !schedule_options(&mut self.output.instructions)
        {
            return false;
        }

        let access_starts = access_calls(&self.output.instructions, owner);
        if access_starts.len() != 4 {
            return false;
        }
        for start in access_starts.into_iter().rev() {
            crate::remove_instruction_retargeting_to_next(self, start);
            crate::remove_instruction_retargeting_to_next(self, start);
            crate::move_instruction_before_retargeting(self, start + 1, start);
            for instruction in &mut self.output.instructions {
                let target = match instruction {
                    Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target } => target,
                    _ => continue,
                };
                if *target == start + 1 {
                    *target = start;
                }
            }
            self.output.instructions[start + 1] = Instruction::AddImmediate {
                d: 5,
                a: owner,
                immediate: 0,
            };
            crate::insert_instruction_retargeting(
                self,
                start + 4,
                Instruction::Or {
                    a: result,
                    s: 3,
                    b: 3,
                },
            );
        }

        let Some(unsupported) = self.output.instructions.windows(2).position(|window| {
            matches!(window[0], Instruction::Branch { .. })
                && matches!(window[1], Instruction::AddImmediate { d: 3, a: 0, immediate: 1795 })
        }).map(|start| start + 1)
        else {
            return false;
        };
        self.output.instructions[unsupported] = Instruction::load_immediate(result, 1795);

        let Some(message_call) = direct_call(&self.output.instructions, "TRKMessageIntoReply") else {
            return false;
        };
        let Some(Instruction::CompareWordImmediate { a: 3, immediate: 0 }) =
            self.output.instructions.get(message_call + 1)
        else {
            return false;
        };
        if !matches!(self.output.instructions.get(message_call - 5), Some(Instruction::CompareWordImmediate { a: 3, immediate: 0 })) {
            return false;
        }
        if let Instruction::CompareWordImmediate { a, .. } =
            &mut self.output.instructions[message_call - 5]
        {
            *a = result;
        }
        if !schedule_error_dispatch(self, message_call + 1, result) {
            return false;
        }

        for start in owner_argument_copies(&self.output.instructions, owner, 5) {
            self.output.instructions[start] = Instruction::AddImmediate {
                d: 5,
                a: owner,
                immediate: 0,
            };
        }
        canonicalize_owner_copies(&mut self.output.instructions, owner);
        true
    }
}

fn schedule_range(generator: &mut Generator) -> bool {
    let Some(start) = generator.output.instructions.windows(4).position(|window| {
        matches!(window[0], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[1], Instruction::Or { a: 4, s: 0, b: 0 })
            && matches!(window[2], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[3], Instruction::CompareWord { a: 4, b: 0 })
    }) else {
        return false;
    };
    let first_offset = match generator.output.instructions[start] {
        Instruction::LoadHalfwordZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    let last_offset = match generator.output.instructions[start + 2] {
        Instruction::LoadHalfwordZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    generator.output.instructions[start] = Instruction::LoadHalfwordZero {
        d: 3,
        a: 1,
        offset: first_offset,
    };
    generator.output.instructions[start + 1] = Instruction::LoadHalfwordZero {
        d: 4,
        a: 1,
        offset: last_offset,
    };
    generator.output.instructions[start + 2] = Instruction::CompareLogicalWord { a: 3, b: 4 };
    crate::remove_instruction_retargeting_to_next(generator, start + 3);
    true
}

fn schedule_options(instructions: &mut [Instruction]) -> bool {
    let Some(start) = instructions.windows(2).position(|window| {
        matches!(window[0], Instruction::LoadByteZero { d: 3, a: 1, .. })
            && matches!(window[1], Instruction::CompareWordImmediate { a: 3, immediate: 2 })
    }) else {
        return false;
    };
    let Some(end) = direct_call(instructions, "TRKTargetAccessDefault") else {
        return false;
    };
    if start >= end {
        return false;
    }
    if let Instruction::LoadByteZero { d, .. } = &mut instructions[start] {
        *d = 0;
    }
    for instruction in &mut instructions[start + 1..end] {
        if let Instruction::CompareWordImmediate { a, immediate } = instruction {
            if *a == 3 && matches!(*immediate, 0 | 2 | 4) {
                *a = 0;
            }
        }
    }
    true
}

fn access_calls(instructions: &[Instruction], owner: u8) -> Vec<usize> {
    instructions.windows(6).enumerate().filter_map(|(start, window)| {
        (matches!(window[0], Instruction::LoadHalfwordZero { d: 3, a: 1, .. })
            && matches!(window[1], Instruction::LoadHalfwordZero { d: 4, a: 1, .. })
            && matches!(window[2], Instruction::Or { a: 5, s, b } if s == owner && b == owner)
            && matches!(window[3], Instruction::AddImmediate { d: 6, a: 1, .. })
            && matches!(window[4], Instruction::AddImmediate { d: 7, a: 0, immediate: 0 })
            && matches!(window[5], Instruction::BranchAndLink { .. }))
        .then_some(start)
    }).collect()
}

fn direct_call(instructions: &[Instruction], expected: &str) -> Option<usize> {
    instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { target } if target == expected)
    })
}

fn owner_argument_copies(instructions: &[Instruction], owner: u8, argument: u8) -> Vec<usize> {
    instructions.iter().enumerate().filter_map(|(index, instruction)| {
        matches!(instruction, Instruction::Or { a, s, b }
            if *a == argument && *s == owner && *b == owner)
        .then_some(index)
    }).collect()
}

fn schedule_error_dispatch(generator: &mut Generator, start: usize, result: u8) -> bool {
    let Some(window) = generator.output.instructions.get(start..start + 31) else {
        return false;
    };
    let expected = [770, 1793, 1794, 1795, 1796, 1797, 1798];
    if !matches!(window[0], Instruction::CompareWordImmediate { a: 3, immediate: 0 })
        || !matches!(window[1], Instruction::BranchConditionalForward { .. })
    {
        return false;
    }
    for (ordinal, immediate) in expected.into_iter().enumerate() {
        let offset = 2 + ordinal * 4;
        if !matches!(window[offset], Instruction::CompareWordImmediate { a: 3, immediate: found } if found == immediate)
            || !matches!(window[offset + 1], Instruction::BranchConditionalForward { .. })
            || !matches!(window[offset + 2], Instruction::AddImmediate { d: 5, a: 0, .. })
            || !matches!(window[offset + 3], Instruction::Branch { .. })
        {
            return false;
        }
    }
    if !matches!(window[30], Instruction::AddImmediate { d: 5, a: 0, immediate: 3 }) {
        return false;
    }

    crate::insert_instruction_retargeting(generator, start + 31, Instruction::load_immediate(5, 3));
    let eq = |target| Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target,
    };
    let ge = |target| Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 0,
        target,
    };
    let compare = |immediate| Instruction::CompareWordImmediate {
        a: result,
        immediate,
    };
    let ack = start + 32;
    let success = start + 36;
    let tree = [
        compare(0),
        eq(success),
        compare(1795),
        eq(start + 17),
        ge(start + 11),
        compare(1793),
        eq(start + 19),
        ge(start + 23),
        compare(770),
        eq(start + 21),
        Instruction::Branch { target: start + 31 },
        compare(1798),
        eq(start + 29),
        ge(start + 31),
        compare(1797),
        ge(start + 27),
        Instruction::Branch { target: start + 25 },
        Instruction::load_immediate(5, 18),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 20),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 2),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 21),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 33),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 34),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 32),
        Instruction::Branch { target: ack },
        Instruction::load_immediate(5, 3),
    ];
    generator.output.instructions[start..start + tree.len()].clone_from_slice(&tree);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access_packet(direction: i16) -> Vec<Instruction> {
        vec![
            Instruction::LoadHalfwordZero { d: 3, a: 1, offset: 20 },
            Instruction::LoadHalfwordZero { d: 4, a: 1, offset: 22 },
            Instruction::Or { a: 5, s: 30, b: 30 },
            Instruction::AddImmediate { d: 6, a: 1, immediate: 24 },
            Instruction::AddImmediate { d: 7, a: 0, immediate: direction },
            Instruction::BranchAndLink { target: "TRKTargetAccessDefault".into() },
        ]
    }

    #[test]
    fn recognizes_only_write_direction_access_packets() {
        assert_eq!(access_calls(&access_packet(0), 30), [0]);
        assert!(access_calls(&access_packet(1), 30).is_empty());
    }

    #[test]
    fn requires_the_retained_owner_register() {
        assert!(access_calls(&access_packet(0), 29).is_empty());
    }
}
