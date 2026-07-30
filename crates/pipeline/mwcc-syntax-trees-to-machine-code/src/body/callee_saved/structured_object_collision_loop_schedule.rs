//! Entry scheduling for pairwise object collision loops.
//!
//! The saved-home planner establishes the semantic roles. This pass then
//! reproduces MWCC's latency schedule: copy incoming objects in source order,
//! initialize the sticky flag, split the global cursor load through r3, and
//! hoist the invariant owner-member address above the loop-entry branch.

use super::structured_object_collision_loop_layout::StructuredObjectCollisionEntryHomes;
#[allow(unused_imports)]
use super::*;

const ENTRY_SCHEDULE: [usize; 8] = [1, 0, 3, 4, 2, 5, 7, 6];

pub(super) fn schedule_object_collision_loop_entry(
    generator: &mut Generator,
    homes: StructuredObjectCollisionEntryHomes,
) -> bool {
    let Some(start) = object_collision_entry(&generator.output.instructions, &homes) else {
        return false;
    };
    let old_receiver = start + 7;
    let body_start = start + ENTRY_SCHEDULE.len();
    for instruction in &mut generator.output.instructions {
        match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target == old_receiver =>
            {
                *target = body_start;
            }
            _ => {}
        }
    }

    let original = generator.output.instructions[start..start + ENTRY_SCHEDULE.len()].to_vec();
    let mut permutation: Vec<_> = (0..generator.output.instructions.len()).collect();
    for (destination, source) in ENTRY_SCHEDULE.into_iter().enumerate() {
        generator.output.instructions[start + destination] = original[source].clone();
        permutation[start + source] = start + destination;
    }
    crate::remap_instruction_indices(generator, &permutation);

    generator.output.instructions[start] =
        Instruction::move_register(homes.owner_parameter, homes.owner_incoming);
    generator.output.instructions[start + 1] =
        Instruction::move_register(homes.other_parameter, homes.other_incoming);
    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start + 3] else {
        unreachable!("the global cursor load was matched")
    };
    *d = Eabi::FIRST_GENERAL_ARGUMENT;
    let Instruction::LoadWord { a, .. } = &mut generator.output.instructions[start + 5] else {
        unreachable!("the cursor member load was matched")
    };
    *a = Eabi::FIRST_GENERAL_ARGUMENT;
    true
}

impl Generator {
    pub(crate) fn finalize_structured_object_collision_loop_entry(&mut self) {
        if !self.structured_object_collision_loop_entry {
            return;
        }
        let Some(start) = allocated_object_collision_entry(&self.output.instructions) else {
            return;
        };
        for index in [start, start + 1] {
            let Instruction::AddImmediate { d, a, immediate: 0 } = self.output.instructions[index]
            else {
                unreachable!("the allocated entry copy was matched")
            };
            self.output.instructions[index] = Instruction::move_register(d, a);
        }
        self.move_instruction_before(start + 7, start + 6);
    }
}

fn allocated_object_collision_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(9).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: owner_parameter,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: other_parameter,
                    a: 4,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    a: 0,
                    immediate: 0,
                    ..
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: owner,
                    a: owner_base,
                    ..
                },
                Instruction::LoadWord { a: 3, .. },
                Instruction::AddImmediate {
                    a: receiver_base,
                    ..
                },
                Instruction::LoadFloatSingle { .. },
                Instruction::Branch { .. },
            ] if owner_parameter != other_parameter
                && owner_base == owner_parameter
                && receiver_base == owner
        )
    })
}

fn object_collision_entry(
    instructions: &[Instruction],
    homes: &StructuredObjectCollisionEntryHomes,
) -> Option<usize> {
    instructions
        .windows(ENTRY_SCHEDULE.len())
        .position(|window| {
            matches!(
                window,
                [
                Instruction::Or {
                    a: other_parameter,
                    s: other_incoming,
                    b: other_incoming_again,
                },
                Instruction::Or {
                    a: owner_parameter,
                    s: owner_incoming,
                    b: owner_incoming_again,
                },
                    Instruction::LoadWord {
                        d: owner,
                        a: owner_base,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: flag,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::LoadWord {
                        d: cursor_global,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::LoadWord {
                        d: cursor_member,
                        a: cursor_base,
                        ..
                    },
                Instruction::Branch { .. },
                    Instruction::AddImmediate {
                        d: receiver,
                        a: receiver_base,
                        ..
                    },
            ] if *other_parameter == homes.other_parameter
                && *other_incoming == homes.other_incoming
                && *other_incoming_again == homes.other_incoming
                && *owner_parameter == homes.owner_parameter
                && *owner_incoming == homes.owner_incoming
                && *owner_incoming_again == homes.owner_incoming
                    && *owner == homes.owner
                    && *owner_base == homes.owner_parameter
                    && *flag == homes.flag
                    && *cursor_global == homes.cursor
                    && *cursor_member == homes.cursor
                    && *cursor_base == homes.cursor
                    && *receiver == homes.receiver
                    && *receiver_base == homes.owner
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homes() -> StructuredObjectCollisionEntryHomes {
        StructuredObjectCollisionEntryHomes {
            owner_parameter: 26,
            other_parameter: 27,
            owner: 30,
            flag: 28,
            cursor: 29,
            receiver: 31,
            owner_incoming: 3,
            other_incoming: 4,
        }
    }

    #[test]
    fn recognizes_the_unscheduled_object_collision_entry() {
        let homes = homes();
        let instructions = vec![
            Instruction::move_register(27, 4),
            Instruction::move_register(26, 3),
            Instruction::LoadWord {
                d: 30,
                a: 26,
                offset: 44,
            },
            Instruction::load_immediate(28, 0),
            Instruction::LoadWord {
                d: 29,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 29,
                a: 29,
                offset: 32,
            },
            Instruction::Branch { target: 20 },
            Instruction::AddImmediate {
                d: 31,
                a: 30,
                immediate: 708,
            },
        ];

        assert_eq!(object_collision_entry(&instructions, &homes), Some(0));
    }

    #[test]
    fn recognizes_the_allocated_object_collision_entry() {
        let instructions = vec![
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 27,
                a: 4,
                immediate: 0,
            },
            Instruction::load_immediate(28, 0),
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 30,
                a: 26,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 29,
                a: 3,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 30,
                immediate: 708,
            },
            Instruction::LoadFloatSingle {
                d: 31,
                a: 0,
                offset: 0,
            },
            Instruction::Branch { target: 20 },
        ];

        assert_eq!(allocated_object_collision_entry(&instructions), Some(0));
    }
}
