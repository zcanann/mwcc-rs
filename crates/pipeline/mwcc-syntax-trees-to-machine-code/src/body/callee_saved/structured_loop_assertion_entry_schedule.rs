//! Incoming-receiver alias scheduling for assertion-bearing structured loops.
//!
//! The entry member load can still read the incoming `r3` while the independent
//! saved-home copy fills its latency slot. The first virtual dispatch can then
//! consume the same untouched `r3` directly, after which later receiver uses
//! continue from the saved home.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_loop_assertion_entry_alias(&mut self) {
        if self.loop_assertion_string_highs.len() != 2 {
            return;
        }
        let Some(removed) = schedule_entry_alias(&mut self.output.instructions) else {
            return;
        };
        self.labels.removed(removed, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > removed {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > removed =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }
}

fn schedule_entry_alias(instructions: &mut Vec<Instruction>) -> Option<usize> {
    let start = instructions.windows(7).position(|window| {
        matches!(
            window,
            [
                Instruction::Or {
                    a: home,
                    s: 3,
                    b: 3,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: load_base,
                    ..
                },
                Instruction::CompareWordImmediate { a: 0, .. },
                Instruction::BranchConditionalForward { .. },
                Instruction::Or {
                    a: 3,
                    s: call_source,
                    b: call_source_b,
                },
                Instruction::LoadWord {
                    d: 12,
                    a: dispatch_base,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 12,
                    a: 12,
                    ..
                },
            ] if home == load_base
                && home == call_source
                && call_source == call_source_b
                && home == dispatch_base
        )
    })?;

    let home_copy = instructions[start].clone();
    let Instruction::LoadWord { d, offset, .. } = instructions[start + 1] else {
        unreachable!("entry-alias recognition guarantees a load");
    };
    instructions[start] = Instruction::LoadWord {
        d,
        a: 3,
        offset,
    };
    instructions[start + 1] = home_copy;
    instructions.remove(start + 4);
    let Instruction::LoadWord { a, .. } = &mut instructions[start + 4] else {
        unreachable!("entry-alias recognition guarantees a dispatch load");
    };
    *a = 3;
    Some(start + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> Vec<Instruction> {
        vec![
            Instruction::move_register(26, 3),
            Instruction::LoadWord {
                d: 0,
                a: 26,
                offset: 20,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 9,
            },
            Instruction::move_register(3, 26),
            Instruction::LoadWord {
                d: 12,
                a: 26,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 12,
                a: 12,
                offset: 12,
            },
        ]
    }

    #[test]
    fn preserves_the_incoming_receiver_through_the_first_dispatch() {
        let mut instructions = stream();

        assert_eq!(schedule_entry_alias(&mut instructions), Some(4));
        assert!(matches!(
            instructions.as_slice(),
            [
                Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: 20,
                },
                Instruction::Or {
                    a: 26,
                    s: 3,
                    b: 3,
                },
                Instruction::CompareWordImmediate { a: 0, immediate: 1 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadWord {
                    d: 12,
                    a: 3,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 12,
                    a: 12,
                    offset: 12,
                },
            ]
        ));
    }

    #[test]
    fn rejects_a_dispatch_that_does_not_use_the_saved_receiver() {
        let mut instructions = stream();
        let Instruction::LoadWord { a, .. } = &mut instructions[5] else {
            unreachable!()
        };
        *a = 27;

        assert_eq!(schedule_entry_alias(&mut instructions), None);
    }
}
