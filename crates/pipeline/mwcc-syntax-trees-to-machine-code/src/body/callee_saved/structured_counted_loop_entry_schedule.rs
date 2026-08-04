//! Entry issue order for dense counted loops with an automatic double table.
//!
//! Generic scheduling keeps the table loads, frame stores, retained-parameter
//! copies, and `memset` size calculation in four contiguous packets. MWCC
//! interleaves those independent packets to cover load latency. Recognition is
//! intentionally physical-shape independent but requires semantic provenance
//! from [`super::structured_counted_loop`].

#[allow(unused_imports)]
use super::*;

const ENTRY_LEN: usize = 30;
const ENTRY_ORDER: [usize; ENTRY_LEN] = [
    0, 1, 22, 20, 2, 23, 3, 18, 4, 24, 5, 21, 6, 19, 7, 17, 8, 26, 9, 25, 27, 10,
    11, 12, 13, 14, 15, 16, 28, 29,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntryPlan {
    start: usize,
    pool_base: u8,
    midpoint: u8,
}

impl Generator {
    pub(crate) fn schedule_dense_counted_loop_entry(&mut self) -> bool {
        if !self.structured_dense_counted_loop_entry_owner {
            return false;
        }
        let Some(plan) = locate_entry(&self.output.instructions) else {
            return false;
        };
        let intermediate = self.fresh_virtual_general_preferring(8);
        self.prefer_virtual_general(plan.pool_base, 9);
        self.prefer_virtual_general(plan.midpoint, 5);

        let mut old = self.output.instructions[plan.start..plan.start + ENTRY_LEN].to_vec();
        let Instruction::AddImmediate { d, a, .. } = &mut old[22] else {
            unreachable!("the counted entry midpoint was recognized as addi")
        };
        *d = intermediate;
        *a = 6;
        let Instruction::ShiftRightLogicalImmediate { s, .. } = &mut old[23] else {
            unreachable!("the counted entry midpoint was recognized as srwi")
        };
        *s = intermediate;
        let Instruction::Add { b, .. } = &mut old[24] else {
            unreachable!("the counted entry midpoint was recognized as add")
        };
        *b = intermediate;

        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in ENTRY_ORDER.into_iter().enumerate() {
            self.output.instructions[plan.start + new_relative] = old[old_relative].clone();
            permutation[plan.start + old_relative] = plan.start + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }
}

fn locate_entry(instructions: &[Instruction]) -> Option<EntryPlan> {
    instructions
        .windows(ENTRY_LEN)
        .enumerate()
        .find_map(|(start, window)| recognize_entry(window).map(|plan| EntryPlan { start, ..plan }))
}

fn recognize_entry(window: &[Instruction]) -> Option<EntryPlan> {
    let Instruction::AddImmediateShifted {
        d: pool_base,
        a: 0,
        immediate: 0,
    } = window[0]
    else {
        return None;
    };
    let Instruction::LoadFloatDoubleWithUpdate {
        d: first_float,
        a,
        offset: 0,
    } = window[1]
    else {
        return None;
    };
    if a != pool_base {
        return None;
    }
    let mut floats = [0u8; 8];
    floats[0] = first_float;
    for index in 1..8 {
        let Instruction::LoadFloatDouble { d, a, offset } = window[index + 1] else {
            return None;
        };
        if a != pool_base || offset != i16::try_from(index * 8).ok()? {
            return None;
        }
        floats[index] = d;
    }
    for (index, &float) in floats.iter().enumerate() {
        if !matches!(
            window[index + 9],
            Instruction::StoreFloatDouble { s, a: 1, offset }
                if s == float && offset == i16::try_from((index + 1) * 8).ok()?
        ) {
            return None;
        }
    }

    let incoming = [6, 7, 5, 4, 3];
    let mut homes = [0u8; 5];
    for (index, &source) in incoming.iter().enumerate() {
        let Instruction::Or { a, s, b } = window[index + 17] else {
            return None;
        };
        if s != source || b != source || a == source {
            return None;
        }
        homes[index] = a;
    }
    let Instruction::AddImmediate {
        d: midpoint,
        a,
        immediate: 1,
    } = window[22]
    else {
        return None;
    };
    if a != homes[0]
        || !matches!(
            window[23],
            Instruction::ShiftRightLogicalImmediate { a: 0, s, shift: 31 }
                if s == midpoint
        )
        || !matches!(
            window[24],
            Instruction::Add { d: 0, a: 0, b } if b == midpoint
        )
        || !matches!(
            window[25],
            Instruction::ShiftRightAlgebraicImmediate { a, s: 0, shift: 1 }
                if a == midpoint
        )
        || !matches!(window[26], Instruction::Or { a: 3, s, b } if s == homes[1] && b == homes[1])
        || !matches!(window[27], Instruction::AddImmediate { d: 4, a: 0, immediate: 0 })
        || !matches!(window[28], Instruction::Or { a: 5, s, b } if s == midpoint && b == midpoint)
        || !matches!(&window[29], Instruction::BranchAndLink { target } if target == "memset")
    {
        return None;
    }
    Some(EntryPlan {
        start: 0,
        pool_base,
        midpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Vec<Instruction> {
        let mut instructions = vec![Instruction::AddImmediateShifted {
            d: 65,
            a: 0,
            immediate: 0,
        }];
        instructions.push(Instruction::LoadFloatDoubleWithUpdate {
            d: 32,
            a: 65,
            offset: 0,
        });
        for index in 1..8 {
            instructions.push(Instruction::LoadFloatDouble {
                d: 32 + index as u8,
                a: 65,
                offset: (index * 8) as i16,
            });
        }
        for index in 0..8 {
            instructions.push(Instruction::StoreFloatDouble {
                s: 32 + index as u8,
                a: 1,
                offset: ((index + 1) * 8) as i16,
            });
        }
        for (home, incoming) in [(70, 6), (71, 7), (72, 5), (73, 4), (74, 3)] {
            instructions.push(Instruction::Or {
                a: home,
                s: incoming,
                b: incoming,
            });
        }
        instructions.extend([
            Instruction::AddImmediate {
                d: 80,
                a: 70,
                immediate: 1,
            },
            Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 80,
                shift: 31,
            },
            Instruction::Add {
                d: 0,
                a: 0,
                b: 80,
            },
            Instruction::ShiftRightAlgebraicImmediate {
                a: 80,
                s: 0,
                shift: 1,
            },
            Instruction::Or {
                a: 3,
                s: 71,
                b: 71,
            },
            Instruction::load_immediate(4, 0),
            Instruction::Or {
                a: 5,
                s: 80,
                b: 80,
            },
            Instruction::BranchAndLink {
                target: "memset".into(),
            },
        ]);
        instructions
    }

    #[test]
    fn recognizes_the_dense_double_table_entry() {
        assert_eq!(
            recognize_entry(&entry()),
            Some(EntryPlan {
                start: 0,
                pool_base: 65,
                midpoint: 80,
            })
        );
    }

    #[test]
    fn rejects_a_noncontiguous_table_image() {
        let mut instructions = entry();
        let Instruction::StoreFloatDouble { offset, .. } = &mut instructions[12] else {
            unreachable!()
        };
        *offset += 8;
        assert_eq!(recognize_entry(&instructions), None);
    }

    #[test]
    fn interleaving_starts_with_the_independent_midpoint_and_flag_copy() {
        assert_eq!(&ENTRY_ORDER[..6], &[0, 1, 22, 20, 2, 23]);
        assert_eq!(&ENTRY_ORDER[21..29], &[10, 11, 12, 13, 14, 15, 16, 28]);
    }
}
