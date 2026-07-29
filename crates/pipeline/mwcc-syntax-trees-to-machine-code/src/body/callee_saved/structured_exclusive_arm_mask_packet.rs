//! Wide-mask arm scheduling for dense path-colored conditional bodies.
//!
//! The mask arm has two related packets. Its guard indexes a byte table before
//! testing a retained 64-bit input, and its body gathers an index, a halfword,
//! and two members from a shared global root. MWCC assigns the short-lived
//! lanes to volatile registers and interleaves the independent loads before
//! committing the retained values to r26-r28.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_mask_packet(&mut self) {
        if let Some((permutation, start)) = rewrite_mask_guard(&mut self.output.instructions) {
            self.labels.moved_before(start + 6, start + 5);
            crate::remap_instruction_indices(self, &permutation);
        }
        if let Some((permutation, start)) = rewrite_mask_body(&mut self.output.instructions) {
            for (from, to) in [
                (start + 1, start),
                (start + 4, start + 1),
                (start + 3, start + 2),
                (start + 6, start + 5),
                (start + 7, start + 6),
                (start + 24, start + 21),
            ] {
                self.labels.moved_before(from, to);
            }
            crate::remap_instruction_indices(self, &permutation);
        }
    }
}

fn rewrite_mask_guard(instructions: &mut [Instruction]) -> Option<(Vec<usize>, usize)> {
    let start = instructions.windows(7).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadByteZero {
                    d: index,
                    offset: 3,
                    ..
                },
                Instruction::Add {
                    d: 3,
                    b: index_source,
                    ..
                },
                Instruction::LoadByteZero {
                    d: 3,
                    a: 3,
                    offset: 824,
                },
                Instruction::BranchAndLink { .. },
                Instruction::Or { a: result, s: 3, b: 3 }
                | Instruction::AddImmediate {
                    d: result,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: zero,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: mask,
                    a: 0,
                    immediate: 128,
                },
            ] if index == index_source && result != zero && zero != mask
        )
    })?;

    let Instruction::LoadByteZero { a, offset, .. } = instructions[start] else {
        unreachable!("the table index was recognized above");
    };
    instructions[start] = Instruction::LoadByteZero { d: 0, a, offset };
    let Instruction::Add { d, a, .. } = instructions[start + 1] else {
        unreachable!("the indexed add was recognized above");
    };
    instructions[start + 1] = Instruction::Add { d, a, b: 0 };

    let original = instructions[start..start + 7].to_vec();
    for (new_offset, old_offset) in [0, 1, 2, 3, 4, 6, 5].into_iter().enumerate() {
        instructions[start + new_offset] = original[old_offset].clone();
    }
    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    permutation[start + 5] = start + 6;
    permutation[start + 6] = start + 5;
    Some((permutation, start))
}

fn rewrite_mask_body(instructions: &mut [Instruction]) -> Option<(Vec<usize>, usize)> {
    let start = instructions.windows(26).position(|window| {
        matches!(
            &window[..9],
            [
                Instruction::LoadHalfwordZero {
                    d: halfword,
                    offset: 4,
                    ..
                },
                Instruction::LoadByteZero {
                    d: index,
                    offset: 3,
                    ..
                },
                Instruction::Add {
                    d: entry,
                    b: index_source,
                    ..
                },
                Instruction::LoadByteZero {
                    d: entry_value,
                    a: entry_base,
                    offset: 824,
                },
                Instruction::LoadWord {
                    d: root,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: object,
                    a: object_root,
                    offset: 44,
                },
                Instruction::LoadWord {
                    d: buffer,
                    a: buffer_root,
                    offset: 40,
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreFloatSingle {
                    s: 0,
                    a: store_base,
                    offset: 12,
                },
            ] if index == index_source
                && entry == entry_value
                && entry == entry_base
                && root == object_root
                && root == buffer_root
                && object == store_base
                && halfword != entry
                && object != buffer
        ) && matches!(
            &window[9..22],
            [
                Instruction::BranchAndLink { .. },
                Instruction::BranchAndLink { .. },
                Instruction::BranchAndLink { .. },
                Instruction::StoreWord { .. },
                Instruction::ClearLeftImmediate { a: 3, .. },
                Instruction::Or { a: 4, .. },
                Instruction::BranchAndLink { .. },
                Instruction::Or { a: _call_result, s: 3, b: 3 }
                | Instruction::AddImmediate {
                    d: _call_result,
                    a: 3,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::Or { a: 3, .. }
                | Instruction::AddImmediate {
                    d: 3,
                    immediate: 0,
                    ..
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    immediate: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 60,
                },
            ]
        ) && matches!(
            &window[22..26],
            [
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 13,
                },
                Instruction::AddImmediate {
                    d: 6,
                    a: 0,
                    immediate: -1,
                },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { .. },
            ]
        )
    })?;

    let original = instructions[start..start + 9].to_vec();
    let [
        Instruction::LoadHalfwordZero { a: state, .. },
        Instruction::LoadByteZero { a: index_base, .. },
        Instruction::Add { a: table, .. },
        _,
        _,
        _,
        _,
        float_literal,
        _,
    ] = original.as_slice()
    else {
        unreachable!("the mask body was recognized above");
    };
    let float_literal = float_literal.clone();
    let state = *state;
    let index_base = *index_base;
    let table = *table;

    let replacement = [
        Instruction::LoadByteZero {
            d: 0,
            a: index_base,
            offset: 3,
        },
        Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        },
        Instruction::Add {
            d: 3,
            a: table,
            b: 0,
        },
        Instruction::LoadHalfwordZero {
            d: 27,
            a: state,
            offset: 4,
        },
        Instruction::LoadByteZero {
            d: 28,
            a: 3,
            offset: 824,
        },
        Instruction::LoadWord {
            d: 26,
            a: 4,
            offset: 40,
        },
        float_literal,
        Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: 44,
        },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 3,
            offset: 12,
        },
    ];
    instructions[start..start + 9].clone_from_slice(&replacement);

    instructions[start + 13] = Instruction::AddImmediate {
        d: 3,
        a: 28,
        immediate: 0,
    };
    instructions[start + 14] = Instruction::AddImmediate {
        d: 4,
        a: 27,
        immediate: 0,
    };
    instructions[start + 16] = Instruction::AddImmediate {
        d: 27,
        a: 3,
        immediate: 0,
    };
    instructions[start + 18] = Instruction::Or {
        a: 3,
        s: 27,
        b: 27,
    };
    instructions[start + 20] = Instruction::AddImmediate {
        d: 3,
        a: 26,
        immediate: 0,
    };
    let tail = instructions[start + 21..start + 25].to_vec();
    for (new_offset, old_offset) in [3, 0, 1, 2].into_iter().enumerate() {
        instructions[start + 21 + new_offset] = tail[old_offset].clone();
    }

    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    for (old_offset, new_offset) in [3, 0, 2, 4, 1, 7, 5, 6, 8].into_iter().enumerate() {
        permutation[start + old_offset] = start + new_offset;
    }
    permutation[start + 21] = start + 22;
    permutation[start + 22] = start + 23;
    permutation[start + 23] = start + 24;
    permutation[start + 24] = start + 21;
    Some((permutation, start))
}

#[cfg(test)]
mod tests {
    use super::rewrite_mask_guard;
    use mwcc_machine_code::Instruction;

    #[test]
    fn indexes_through_r0_and_materializes_the_mask_before_zero() {
        let index = mwcc_vreg::VIRTUAL_BASE;
        let result = index + 1;
        let zero = index + 2;
        let mask = index + 3;
        let mut instructions = vec![
            Instruction::LoadByteZero {
                d: index,
                a: 31,
                offset: 3,
            },
            Instruction::Add {
                d: 3,
                a: 30,
                b: index,
            },
            Instruction::LoadByteZero {
                d: 3,
                a: 3,
                offset: 824,
            },
            Instruction::BranchAndLink {
                target: "read".into(),
            },
            Instruction::move_register(result, 3),
            Instruction::load_immediate(zero, 0),
            Instruction::load_immediate(mask, 128),
        ];

        let (permutation, start) = rewrite_mask_guard(&mut instructions).unwrap();

        assert_eq!(start, 0);
        assert_eq!(permutation, [0, 1, 2, 3, 4, 6, 5]);
        assert!(matches!(
            instructions[0],
            Instruction::LoadByteZero { d: 0, .. }
        ));
        assert!(matches!(
            instructions[1],
            Instruction::Add { b: 0, .. }
        ));
        assert_eq!(instructions[5], Instruction::load_immediate(mask, 128));
        assert_eq!(instructions[6], Instruction::load_immediate(zero, 0));
    }
}
