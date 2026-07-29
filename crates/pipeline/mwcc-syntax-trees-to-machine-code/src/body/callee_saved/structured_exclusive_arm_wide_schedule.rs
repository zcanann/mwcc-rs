//! Wide call-result scheduling for dense path-colored conditional bodies.
//!
//! A 32-bit call result assigned through a 64-bit global keeps a retained zero
//! high lane. MWCC overlaps address formation with that pair's mask test, then
//! delays the high-word store until CR0 has consumed the retained zero.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_wide_snapshot(&mut self) {
        let Some((permutation, start)) = rewrite_wide_snapshot(&mut self.output.instructions) else {
            return;
        };
        for (from, to) in [
            (start + 2, start + 1),
            (start + 3, start + 2),
            (start + 7, start + 4),
            (start + 8, start + 5),
            (start + 9, start + 6),
            (start + 10, start + 9),
            (start + 11, start + 10),
            (start + 12, start + 11),
        ] {
            self.labels.moved_before(from, to);
        }
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn rewrite_wide_snapshot(instructions: &mut [Instruction]) -> Option<(Vec<usize>, usize)> {
    let start = instructions.windows(13).position(recognizes_wide_snapshot)?;
    let window = &instructions[start..start + 13];
    let (low, high, address, mask, tested_low) = match window {
        [
            Instruction::BranchAndLink { .. },
            Instruction::Or { a: low, s: 3, b: 3 }
            | Instruction::AddImmediate {
                d: low,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: high,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: address,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate { .. },
            Instruction::StoreWord { .. },
            Instruction::StoreWord { .. },
            Instruction::AddImmediate {
                d: mask,
                a: 0,
                immediate: 32,
            },
            Instruction::And { .. },
            Instruction::And { a: tested_low, .. },
            Instruction::Xor { .. },
            Instruction::Xor { .. },
            Instruction::OrRecord { .. },
        ] => (*low, *high, *address, *mask, *tested_low),
        _ => unreachable!("the wide snapshot was recognized above"),
    };

    let order = [0, 2, 3, 1, 7, 8, 9, 4, 5, 10, 11, 12, 6];
    let original = instructions[start..start + 13].to_vec();
    for (new_offset, old_offset) in order.into_iter().enumerate() {
        instructions[start + new_offset] = original[old_offset].clone();
    }
    instructions[start + 2] = Instruction::load_immediate_shifted(4, 0);
    instructions[start + 7] = Instruction::AddImmediate {
        d: 4,
        a: 4,
        immediate: 0,
    };
    let Instruction::StoreWord { a: low_base, .. } = &mut instructions[start + 8] else {
        unreachable!("the low store was recognized above");
    };
    *low_base = 4;
    let Instruction::StoreWord {
        a: high_base, ..
    } = &mut instructions[start + 12]
    else {
        unreachable!("the high store was recognized above");
    };
    *high_base = 4;

    debug_assert!(matches!(
        instructions[start + 1],
        Instruction::AddImmediate { d, a: 0, immediate: 0 } if d == high
    ));
    debug_assert!(matches!(
        instructions[start + 3],
        Instruction::Or { a, s: 3, b: 3 }
            | Instruction::AddImmediate { d: a, a: 3, immediate: 0 }
            if a == low
    ));
    debug_assert!(matches!(
        instructions[start + 4],
        Instruction::AddImmediate { d, a: 0, immediate: 32 } if d == mask
    ));
    debug_assert!(matches!(
        instructions[start + 6],
        Instruction::And { a, .. } if a == tested_low
    ));
    let _ = address;

    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    for (new_offset, old_offset) in order.into_iter().enumerate() {
        permutation[start + old_offset] = start + new_offset;
    }
    Some((permutation, start))
}

fn recognizes_wide_snapshot(window: &[Instruction]) -> bool {
    let [
        Instruction::BranchAndLink { .. },
        Instruction::Or { a: low, s: 3, b: 3 }
        | Instruction::AddImmediate {
            d: low,
            a: 3,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: high,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediateShifted {
            d: address,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: low_address,
            a: address_base,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: low_store,
            a: low_store_base,
            offset: 12,
        },
        Instruction::StoreWord {
            s: high_store,
            a: high_store_base,
            offset: 8,
        },
        Instruction::AddImmediate {
            d: mask,
            a: 0,
            immediate: 32,
        },
        Instruction::And {
            a: tested_high,
            s: high_and_left,
            b: high_and_right,
        },
        Instruction::And {
            a: tested_low,
            s: low_and,
            b: mask_source,
        },
        Instruction::Xor {
            a: low_xor,
            s: low_xor_source,
            b: low_zero,
        },
        Instruction::Xor {
            a: high_xor,
            s: high_xor_source,
            b: high_zero,
        },
        Instruction::OrRecord {
            a: 0,
            s: or_left,
            b: or_right,
        },
    ] = window
    else {
        return false;
    };
    low == low_store
        && low == low_and
        && high == high_store
        && high == high_and_left
        && high == high_and_right
        && high == low_zero
        && high == high_zero
        && address == address_base
        && low_address == low_store_base
        && low_address == high_store_base
        && mask == mask_source
        && tested_low == low_xor_source
        && low_xor == or_left
        && tested_high == high_xor_source
        && high_xor == or_right
}

#[cfg(test)]
mod tests {
    use super::rewrite_wide_snapshot;
    use mwcc_machine_code::Instruction;

    #[test]
    fn overlaps_wide_snapshot_store_with_its_first_mask_test() {
        let low = mwcc_vreg::VIRTUAL_BASE;
        let high = low + 1;
        let address = low + 2;
        let mask = low + 3;
        let tested_high = low + 4;
        let tested_low = low + 5;
        let low_xor = low + 6;
        let high_xor = low + 7;
        let mut instructions = vec![
            Instruction::BranchAndLink {
                target: "read".into(),
            },
            Instruction::Or {
                a: low,
                s: 3,
                b: 3,
            },
            Instruction::load_immediate(high, 0),
            Instruction::load_immediate_shifted(address, 0),
            Instruction::AddImmediate {
                d: address,
                a: address,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: low,
                a: address,
                offset: 12,
            },
            Instruction::StoreWord {
                s: high,
                a: address,
                offset: 8,
            },
            Instruction::load_immediate(mask, 32),
            Instruction::And {
                a: tested_high,
                s: high,
                b: high,
            },
            Instruction::And {
                a: tested_low,
                s: low,
                b: mask,
            },
            Instruction::Xor {
                a: low_xor,
                s: tested_low,
                b: high,
            },
            Instruction::Xor {
                a: high_xor,
                s: tested_high,
                b: high,
            },
            Instruction::OrRecord {
                a: 0,
                s: low_xor,
                b: high_xor,
            },
        ];

        let (permutation, start) = rewrite_wide_snapshot(&mut instructions).unwrap();

        assert_eq!(start, 0);
        assert_eq!(
            permutation,
            [0, 3, 1, 2, 7, 8, 12, 4, 5, 6, 9, 10, 11]
        );
        assert_eq!(instructions[1], Instruction::load_immediate(high, 0));
        assert_eq!(instructions[2], Instruction::load_immediate_shifted(4, 0));
        assert!(matches!(instructions[3], Instruction::Or { a, .. } if a == low));
        assert_eq!(instructions[4], Instruction::load_immediate(mask, 32));
        assert!(matches!(
            instructions[7],
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 }
        ));
        assert!(matches!(
            instructions[8],
            Instruction::StoreWord { s, a: 4, offset: 12 } if s == low
        ));
        assert!(matches!(
            instructions[12],
            Instruction::StoreWord { s, a: 4, offset: 8 } if s == high
        ));
    }
}
