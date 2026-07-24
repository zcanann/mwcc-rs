//! Final load/scale reuse for a structured packet-loop setup word.
//!
//! Allocation exposes one compact physical-register window where the same
//! halfword is loaded twice and its one-bit scale is also recomputed. MWCC
//! keeps both values in the second load's lane. This pass owns only the fully
//! linked F5.88 packed-word setup, after allocation has made lane reuse explicit.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_structured_loop_packet_setup(&mut self) {
        let Some((first_load, duplicate_load, duplicate_shift)) =
            coalesce_packet_setup_registers(&mut self.output.instructions)
        else {
            return;
        };
        debug_assert!(first_load < duplicate_load && duplicate_load < duplicate_shift);
        self.remove_packet_setup_instruction(duplicate_shift);
        self.remove_packet_setup_instruction(duplicate_load);
    }

    fn remove_packet_setup_instruction(&mut self, index: usize) {
        let old_len = self.output.instructions.len();
        self.output.instructions.remove(index);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < index {
                    old
                } else if old == index {
                    index.saturating_sub(1)
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn coalesce_packet_setup_registers(
    instructions: &mut [Instruction],
) -> Option<(usize, usize, usize)> {
    let start = instructions
        .windows(10)
        .position(is_redundant_packet_setup)?;
    let [first_load, add, _duplicate_load, _other_scale, first_scale, subtract, _bias, _packed, _command, _duplicate_scale] =
        &mut instructions[start..start + 10]
    else {
        unreachable!("the packet setup window was matched")
    };

    let Instruction::LoadHalfwordZero {
        d: first,
        a: _,
        offset: _,
    } = first_load
    else {
        unreachable!()
    };
    let Instruction::Add { a, b, .. } = add else {
        unreachable!()
    };
    let Instruction::ShiftLeftImmediate {
        a: scaled,
        s: source,
        ..
    } = first_scale
    else {
        unreachable!()
    };
    let replacement = *source;
    let temporary = *scaled;

    *first = replacement;
    if *a == temporary {
        *a = replacement;
    } else if *b == temporary {
        *b = replacement;
    } else {
        unreachable!("the first load fed the matched add")
    }
    *scaled = replacement;
    let Instruction::SubtractFrom { a, b, .. } = subtract else {
        unreachable!()
    };
    if *a == temporary {
        *a = replacement;
    } else if *b == temporary {
        *b = replacement;
    } else {
        unreachable!("the first scale fed the matched subtraction")
    }

    Some((start, start + 2, start + 9))
}

fn is_redundant_packet_setup(window: &[Instruction]) -> bool {
    let [Instruction::LoadHalfwordZero {
        d: first,
        a: base,
        offset,
    }, Instruction::Add {
        d: add_result,
        a: add_a,
        b: add_b,
    }, Instruction::LoadHalfwordZero {
        d: retained,
        a: second_base,
        offset: second_offset,
    }, Instruction::ShiftLeftImmediate {
        a: other_scaled,
        shift: 1,
        ..
    }, Instruction::ShiftLeftImmediate {
        a: temporary,
        s: scale_source,
        shift: 1,
    }, Instruction::SubtractFrom {
        d: difference,
        a: subtract_a,
        b: subtract_b,
    }, Instruction::AddImmediate {
        d: biased,
        a: bias_source,
        immediate: 7,
    }, Instruction::RotateAndMask {
        a: packed,
        s: packed_source,
        shift: 6,
        begin: 14,
        end: 22,
    }, Instruction::OrImmediateShifted {
        a: command,
        s: command_source,
        immediate: 62856,
    }, Instruction::ShiftLeftImmediate {
        a: late_scaled,
        s: late_source,
        shift: 1,
    }] = window
    else {
        return false;
    };

    base == second_base
        && offset == second_offset
        && first != retained
        && temporary == first
        && retained == scale_source
        && retained == late_scaled
        && retained == late_source
        && other_scaled == difference
        && ((*add_a == *first && *add_b != *retained) || (*add_b == *first && *add_a != *retained))
        && add_result != retained
        && ((*subtract_a == *other_scaled && *subtract_b == *temporary)
            || (*subtract_b == *other_scaled && *subtract_a == *temporary))
        && bias_source == difference
        && packed_source == biased
        && command_source == packed
        && command == difference
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Vec<Instruction> {
        vec![
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 14,
                offset: 4,
            },
            Instruction::Add { d: 9, a: 0, b: 16 },
            Instruction::LoadHalfwordZero {
                d: 10,
                a: 14,
                offset: 4,
            },
            Instruction::ShiftLeftImmediate {
                a: 11,
                s: 3,
                shift: 1,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 10,
                shift: 1,
            },
            Instruction::SubtractFrom { d: 11, a: 11, b: 0 },
            Instruction::AddImmediate {
                d: 0,
                a: 11,
                immediate: 7,
            },
            Instruction::RotateAndMask {
                a: 0,
                s: 0,
                shift: 6,
                begin: 14,
                end: 22,
            },
            Instruction::OrImmediateShifted {
                a: 11,
                s: 0,
                immediate: 62856,
            },
            Instruction::ShiftLeftImmediate {
                a: 10,
                s: 10,
                shift: 1,
            },
        ]
    }

    #[test]
    fn reuses_the_retained_load_lane_for_the_first_add_and_scale() {
        let mut instructions = setup();
        assert_eq!(
            coalesce_packet_setup_registers(&mut instructions),
            Some((0, 2, 9))
        );

        assert!(matches!(
            &instructions[0],
            Instruction::LoadHalfwordZero { d: 10, .. }
        ));
        assert!(matches!(&instructions[1], Instruction::Add { a: 10, .. }));
        assert!(matches!(
            &instructions[4],
            Instruction::ShiftLeftImmediate { a: 10, s: 10, .. }
        ));
        assert!(matches!(
            &instructions[5],
            Instruction::SubtractFrom { b: 10, .. }
        ));
    }

    #[test]
    fn rejects_different_member_offsets() {
        let mut instructions = setup();
        let Instruction::LoadHalfwordZero { offset, .. } = &mut instructions[2] else {
            unreachable!()
        };
        *offset = 6;

        assert_eq!(coalesce_packet_setup_registers(&mut instructions), None);
    }
}
