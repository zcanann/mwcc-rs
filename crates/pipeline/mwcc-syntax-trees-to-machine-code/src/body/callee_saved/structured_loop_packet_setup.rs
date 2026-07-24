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
        let dead_load = dead_prepacket_member_load(&self.output.instructions, first_load);
        let preserved_load =
            super::structured_loop_packet_load_reuse::preserve_earlier_member_load(
                &mut self.output.instructions,
                first_load,
            );
        debug_assert!(first_load < duplicate_load && duplicate_load < duplicate_shift);
        self.remove_packet_setup_instruction(duplicate_shift);
        self.remove_packet_setup_instruction(duplicate_load);
        if preserved_load {
            self.remove_packet_setup_instruction(first_load);
        }
        if let Some(dead_load) = dead_load {
            self.remove_packet_setup_instruction(dead_load);
        }
        if let Some(high_constant) =
            super::structured_loop_packet_immediates::fold_masked_high_constant(
                &mut self.output.instructions,
            )
        {
            self.remove_packet_setup_instruction(high_constant);
        }
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

fn dead_prepacket_member_load(instructions: &[Instruction], setup: usize) -> Option<usize> {
    let start = setup.checked_sub(18)?;
    let [Instruction::LoadHalfwordZero {
        d: dead,
        a: base,
        offset: width_offset,
    }, Instruction::CompareWordImmediate { a: first_guard, .. }, Instruction::BranchConditionalForward {
        target: first_else, ..
    }, Instruction::Negate {
        d: first_negative,
        a: first_negative_source,
    }, Instruction::AddImmediate {
        d: first_zero,
        a: 0,
        immediate: 0,
    }, Instruction::Branch { target: first_join }, Instruction::AddImmediate {
        d: first_else_zero,
        a: 0,
        immediate: 0,
    }, Instruction::Or {
        a: first_copy,
        s: first_copy_source,
        b: first_copy_other,
    }, Instruction::CompareWordImmediate {
        a: second_guard, ..
    }, Instruction::BranchConditionalForward {
        target: second_else,
        ..
    }, Instruction::Negate {
        d: second_negative,
        a: second_negative_source,
    }, Instruction::AddImmediate {
        d: second_zero,
        a: 0,
        immediate: 0,
    }, Instruction::LoadHalfwordZero {
        d: height,
        a: height_base,
        offset: height_offset,
    }, Instruction::Add {
        d: negative_value,
        a: negative_a,
        b: negative_b,
    }, Instruction::Branch { target: join }, Instruction::LoadHalfwordZero {
        d: positive_value,
        a: positive_base,
        offset: positive_offset,
    }, Instruction::Or {
        a: second_copy,
        s: second_copy_source,
        b: second_copy_other,
    }, Instruction::AddImmediate {
        d: second_else_zero,
        a: 0,
        immediate: 0,
    }] = &instructions[start..setup]
    else {
        return None;
    };

    (*first_else == start + 6
        && *first_join == start + 8
        && *second_else == start + 15
        && *join == setup
        && dead == negative_value
        && dead == positive_value
        && base == height_base
        && base == positive_base
        && height_offset == positive_offset
        && width_offset != height_offset
        && (*negative_a == *height || *negative_b == *height)
        && *first_guard != *dead
        && *first_negative != *dead
        && *first_negative_source != *dead
        && *first_zero != *dead
        && *first_else_zero != *dead
        && *first_copy != *dead
        && *first_copy_source != *dead
        && *first_copy_other != *dead
        && *second_guard != *dead
        && *second_negative != *dead
        && *second_negative_source != *dead
        && *second_zero != *dead
        && *height != *dead
        && *second_copy != *dead
        && *second_copy_source != *dead
        && *second_copy_other != *dead
        && *second_else_zero != *dead)
        .then_some(start)
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

    fn clamp_prefix() -> Vec<Instruction> {
        vec![
            Instruction::LoadHalfwordZero {
                d: 5,
                a: 14,
                offset: 4,
            },
            Instruction::CompareWordImmediate {
                a: 16,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 6,
            },
            Instruction::Negate { d: 3, a: 16 },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::Branch { target: 8 },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::Or { a: 6, s: 16, b: 16 },
            Instruction::CompareWordImmediate {
                a: 17,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 15,
            },
            Instruction::Negate { d: 7, a: 17 },
            Instruction::AddImmediate {
                d: 8,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 14,
                offset: 6,
            },
            Instruction::Add { d: 5, a: 0, b: 17 },
            Instruction::Branch { target: 18 },
            Instruction::LoadHalfwordZero {
                d: 5,
                a: 14,
                offset: 6,
            },
            Instruction::Or { a: 8, s: 17, b: 17 },
            Instruction::AddImmediate {
                d: 7,
                a: 0,
                immediate: 0,
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

    #[test]
    fn recognizes_a_member_load_overwritten_on_both_clamp_paths() {
        let mut instructions = clamp_prefix();
        instructions.extend(setup());

        assert_eq!(dead_prepacket_member_load(&instructions, 18), Some(0));
    }
}
