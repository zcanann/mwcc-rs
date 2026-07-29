//! Repeated object-creation scheduling for dense path-colored arms.
//!
//! Two sibling arms update an optional display object, allocate its replacement,
//! initialize the same float fields, derive a frame position, and submit it.
//! MWCC keeps the display owner in r27 and the new object in r28, while moving
//! independent frame/condition setup ahead of their corresponding packets.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_object_creation(&mut self) {
        while let Some((permutation, start)) =
            rewrite_object_creation_packet(&mut self.output.instructions)
        {
            for (from, to) in [
                (start + 22, start + 12),
                (start + 31, start + 26),
                (start + 31, start + 29),
            ] {
                self.labels.moved_before(from, to);
            }
            crate::remap_instruction_indices(self, &permutation);
        }
    }

    pub(crate) fn finalize_exclusive_arm_copy_encodings(&mut self) {
        finalize_copy_encodings(&mut self.output.instructions);
    }
}

fn finalize_copy_encodings(instructions: &mut [Instruction]) {
    for start in 0..instructions.len().saturating_sub(4) {
        if matches!(
            &instructions[start..start + 5],
            [
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 27,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 26,
                    immediate: 0,
                },
                Instruction::ConditionRegisterClear { d: 6 },
            ]
        ) {
            instructions[start + 1] = Instruction::Or {
                a: 3,
                s: 27,
                b: 27,
            };
        }
        if matches!(
            &instructions[start..start + 5],
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 28,
                    a: 3,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: 28,
                    a: 27,
                    offset: 28,
                },
            ]
        ) {
            instructions[start + 3] = Instruction::Or {
                a: 28,
                s: 3,
                b: 3,
            };
        }
    }
}

fn rewrite_object_creation_packet(
    instructions: &mut [Instruction],
) -> Option<(Vec<usize>, usize)> {
    let start = instructions
        .windows(33)
        .position(recognizes_object_creation_packet)?;
    let original = instructions[start..start + 33].to_vec();

    let owner = match original[1] {
        Instruction::LoadWord { d, .. } => d,
        _ => unreachable!("the owner load was recognized above"),
    };
    let existing = match original[2] {
        Instruction::LoadWord { d, .. } => d,
        _ => unreachable!("the existing object load was recognized above"),
    };
    let created = match original[10] {
        Instruction::Or { a, .. } | Instruction::AddImmediate { d: a, .. } => a,
        _ => unreachable!("the created object copy was recognized above"),
    };
    debug_assert!(owner != existing && owner != created);

    let order: [usize; 33] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 22, 12, 13, 14, 15, 16, 17, 18,
        19, 20, 21, 23, 24, 25, 31, 26, 27, 30, 28, 29, 32,
    ];
    for (new_offset, old_offset) in order.into_iter().enumerate() {
        instructions[start + new_offset] = original[old_offset].clone();
    }

    instructions[start + 1] = Instruction::LoadWord {
        d: 27,
        a: 3,
        offset: 44,
    };
    instructions[start + 2] = Instruction::LoadWord {
        d: 3,
        a: 27,
        offset: 28,
    };
    instructions[start + 3] = Instruction::CompareLogicalWordImmediate {
        a: 3,
        immediate: 0,
    };
    instructions[start + 5] = Instruction::Or { a: 3, s: 3, b: 3 };
    instructions[start + 10] = Instruction::Or {
        a: 28,
        s: 3,
        b: 3,
    };
    instructions[start + 11] = Instruction::StoreWord {
        s: 28,
        a: 27,
        offset: 28,
    };
    for (offset, field_offset) in [(14, 0), (16, 4), (18, 8), (20, 36), (22, 40)] {
        let Instruction::StoreFloatSingle { s, .. } = instructions[start + offset] else {
            unreachable!("the float field store was recognized above");
        };
        instructions[start + offset] = Instruction::StoreFloatSingle {
            s,
            a: 28,
            offset: field_offset,
        };
    }
    let Instruction::LoadHalfwordZero { d, offset, .. } = instructions[start + 23] else {
        unreachable!("the owner halfword was recognized above");
    };
    instructions[start + 23] = Instruction::LoadHalfwordZero {
        d,
        a: 27,
        offset,
    };
    let Instruction::StoreByte { s, offset, .. } = instructions[start + 27] else {
        unreachable!("the created-object flag store was recognized above");
    };
    instructions[start + 27] = Instruction::StoreByte {
        s,
        a: 28,
        offset,
    };
    instructions[start + 28] = Instruction::AddImmediate {
        d: 3,
        a: 28,
        immediate: 0,
    };

    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    for (new_offset, old_offset) in order.into_iter().enumerate() {
        permutation[start + old_offset] = start + new_offset;
    }
    Some((permutation, start))
}

fn recognizes_object_creation_packet(window: &[Instruction]) -> bool {
    let [
        Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        },
        Instruction::LoadWord {
            d: owner,
            a: 3,
            offset: 44,
        },
        Instruction::LoadWord {
            d: existing,
            a: existing_owner,
            offset: 28,
        },
        Instruction::CompareLogicalWordImmediate {
            a: compared,
            immediate: 0,
        },
        Instruction::BranchConditionalForward { .. },
        Instruction::Or {
            a: 3,
            s: existing_argument,
            b: existing_duplicate,
        },
        Instruction::BranchAndLink { .. },
        Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        },
        Instruction::BranchAndLink { .. },
        Instruction::Or {
            a: created,
            s: 3,
            b: 3,
        }
        | Instruction::AddImmediate {
            d: created,
            a: 3,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: stored_created,
            a: store_owner,
            offset: 28,
        },
        Instruction::LoadFloatSingle { d: 0, .. },
        Instruction::StoreFloatSingle {
            s: 0,
            a: first_created,
            offset: 0,
        },
        Instruction::LoadFloatSingle { d: 0, .. },
        Instruction::StoreFloatSingle {
            s: 0,
            a: second_created,
            offset: 4,
        },
        Instruction::LoadFloatSingle { d: 0, .. },
        Instruction::StoreFloatSingle {
            s: 0,
            a: third_created,
            offset: 8,
        },
        Instruction::LoadFloatSingle { d: 0, .. },
        Instruction::StoreFloatSingle {
            s: 0,
            a: fourth_created,
            offset: 36,
        },
        Instruction::LoadFloatSingle { d: 0, .. },
        Instruction::StoreFloatSingle {
            s: 0,
            a: fifth_created,
            offset: 40,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: frame_offset,
        },
        Instruction::LoadHalfwordZero {
            d: 4,
            a: halfword_owner,
            offset: 4,
        },
        Instruction::BranchAndLink { .. },
        Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 1,
        },
        Instruction::StoreByte {
            s: 0,
            a: flagged_created,
            offset: 74,
        },
        Instruction::AddImmediate {
            d: 3,
            a: argument_created,
            immediate: 0,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 0,
            offset: 0,
        },
        Instruction::FloatMove { d: 2, b: 1 },
        Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: argument_frame_offset,
        },
        Instruction::ConditionRegisterSet { d: 6 },
        Instruction::BranchAndLink { .. },
    ] = window
    else {
        return false;
    };
    owner == existing_owner
        && owner == store_owner
        && owner == halfword_owner
        && existing == compared
        && existing == existing_argument
        && existing == existing_duplicate
        && created == stored_created
        && created == first_created
        && created == second_created
        && created == third_created
        && created == fourth_created
        && created == fifth_created
        && created == flagged_created
        && created == argument_created
        && frame_offset == argument_frame_offset
        && matches!(frame_offset, 12 | 16)
}

#[cfg(test)]
mod tests {
    use super::{finalize_copy_encodings, recognizes_object_creation_packet};
    use mwcc_machine_code::Instruction;

    #[test]
    fn rejects_an_incomplete_object_creation_packet() {
        let instructions = vec![Instruction::BranchAndLink {
            target: "allocate".into(),
        }];
        assert!(!recognizes_object_creation_packet(&instructions));
    }

    #[test]
    fn preserves_control_flow_and_created_object_copies_as_mr() {
        let mut instructions = vec![
            Instruction::BranchAndLink {
                target: "pause".into(),
            },
            Instruction::AddImmediate {
                d: 3,
                a: 27,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "resume".into(),
            },
            Instruction::AddImmediate {
                d: 3,
                a: 26,
                immediate: 0,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::load_immediate(3, 0),
            Instruction::load_immediate(4, 0),
            Instruction::BranchAndLink {
                target: "create".into(),
            },
            Instruction::AddImmediate {
                d: 28,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 28,
                a: 27,
                offset: 28,
            },
        ];

        finalize_copy_encodings(&mut instructions);

        assert_eq!(instructions[1], Instruction::move_register(3, 27));
        assert_eq!(instructions[8], Instruction::move_register(28, 3));
    }
}
