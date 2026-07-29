//! Entry scheduling for dense path-colored conditional bodies.
//!
//! Once the receiver has a saved home, MWCC reuses incoming r3 for the data
//! anchor, evaluates the independent narrow guard in r4, and delays the eager
//! receiver-member load until both values are established.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_entry(&mut self) {
        let Some((permutation, start)) = rewrite_entry_window(&mut self.output.instructions) else {
            return;
        };
        self.labels.moved_before(start + 3, start);
        self.labels.moved_before(start + 4, start + 3);
        self.labels.moved_before(start + 10, start + 9);
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn rewrite_entry_window(instructions: &mut [Instruction]) -> Option<(Vec<usize>, usize)> {
    let start = instructions.windows(13).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted {
                    a: 0,
                    immediate: 0,
                    ..
                },
                Instruction::AddImmediate { immediate: 0, .. },
                Instruction::LoadWord { a: 3, .. },
                Instruction::Or { s: 3, b: 3, .. }
                    | Instruction::AddImmediate {
                        a: 3,
                        immediate: 0,
                        ..
                    },
                Instruction::LoadHalfwordZero {
                    a: 0,
                    offset: 0,
                    ..
                },
                Instruction::CompareLogicalWordImmediate { immediate: 0, .. },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate {
                    d: 0,
                    immediate: -1,
                    ..
                },
                Instruction::StoreHalfword { s: 0, a: 0, .. },
                Instruction::AddImmediate {
                    a: 0,
                    immediate: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreHalfword { s: 0, .. },
                Instruction::StoreWord { s: 0, .. },
            ]
        )
    })?;
    let (high, anchor, low_base, eager, offset, guard, compared) =
        match &instructions[start..start + 6] {
            [
                Instruction::AddImmediateShifted { d: high, .. },
                Instruction::AddImmediate {
                    d: anchor,
                    a: low_base,
                    ..
                },
                Instruction::LoadWord {
                    d: eager, offset, ..
                },
                _,
                Instruction::LoadHalfwordZero { d: guard, .. },
                Instruction::CompareLogicalWordImmediate { a: compared, .. },
            ] => (*high, *anchor, *low_base, *eager, *offset, *guard, *compared),
            _ => unreachable!("the entry window was recognized above"),
        };
    let saved_receiver = match instructions[start + 3] {
        Instruction::Or { a, s: 3, b: 3 }
        | Instruction::AddImmediate {
            d: a,
            a: 3,
            immediate: 0,
        } => a,
        _ => unreachable!("the receiver copy was recognized above"),
    };
    let clear_base = match (
        &instructions[start + 9],
        &instructions[start + 11],
        &instructions[start + 12],
    ) {
        (
            Instruction::AddImmediate { d, .. },
            Instruction::StoreHalfword { a: half_base, .. },
            Instruction::StoreWord { a: word_base, .. },
        ) if d == half_base && d == word_base => *d,
        _ => return None,
    };
    if high != low_base
        || guard != compared
        || anchor < mwcc_vreg::VIRTUAL_BASE
        || eager < mwcc_vreg::VIRTUAL_BASE
        || saved_receiver < mwcc_vreg::VIRTUAL_BASE
    {
        return None;
    }

    instructions[start..start + 4].rotate_right(1);
    instructions[start + 3..start + 5].rotate_right(1);
    instructions[start + 1] = Instruction::load_immediate_shifted(3, 0);
    instructions[start + 2] = Instruction::AddImmediate {
        d: anchor,
        a: 3,
        immediate: 0,
    };
    instructions[start + 3] = Instruction::LoadHalfwordZero {
        d: 4,
        a: 0,
        offset: 0,
    };
    instructions[start + 4] = Instruction::LoadWord {
        d: eager,
        a: saved_receiver,
        offset,
    };
    instructions[start + 5] = Instruction::CompareLogicalWordImmediate {
        a: 4,
        immediate: 0,
    };
    let Instruction::AddImmediate { a, .. } = &mut instructions[start + 7] else {
        unreachable!("the guarded decrement was recognized above");
    };
    *a = 4;
    instructions[start + 9..start + 11].rotate_right(1);

    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    for (old_offset, new_offset) in [1, 2, 4, 0, 3, 5].into_iter().enumerate() {
        permutation[start + old_offset] = start + new_offset;
    }
    permutation[start + 9] = start + 10;
    permutation[start + 10] = start + 9;
    debug_assert_eq!(
        instructions[start + 10],
        Instruction::AddImmediate {
            d: clear_base,
            a: 0,
            immediate: 0,
        }
    );
    Some((permutation, start))
}

#[cfg(test)]
mod tests {
    use super::rewrite_entry_window;
    use mwcc_machine_code::Instruction;

    #[test]
    fn saves_receiver_before_reusing_entry_scratch() {
        let anchor = mwcc_vreg::VIRTUAL_BASE;
        let eager = anchor + 1;
        let saved = anchor + 2;
        let mut instructions = vec![
            Instruction::load_immediate_shifted(5, 0),
            Instruction::AddImmediate {
                d: anchor,
                a: 5,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: eager,
                a: 3,
                offset: 44,
            },
            Instruction::AddImmediate {
                d: saved,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadHalfwordZero {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -1,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 5,
                offset: 2,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 4,
            },
        ];

        let (permutation, start) = rewrite_entry_window(&mut instructions).unwrap();

        assert_eq!(start, 0);
        assert_eq!(
            permutation,
            [1, 2, 4, 0, 3, 5, 6, 7, 8, 10, 9, 11, 12]
        );
        assert_eq!(
            instructions,
            [
                Instruction::AddImmediate {
                    d: saved,
                    a: 3,
                    immediate: 0,
                },
                Instruction::load_immediate_shifted(3, 0),
                Instruction::AddImmediate {
                    d: anchor,
                    a: 3,
                    immediate: 0,
                },
                Instruction::LoadHalfwordZero {
                    d: 4,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: eager,
                    a: saved,
                    offset: 44,
                },
                Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 8,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 4,
                    immediate: -1,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: 5,
                    offset: 2,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 5,
                    offset: 4,
                },
            ]
        );
    }
}
