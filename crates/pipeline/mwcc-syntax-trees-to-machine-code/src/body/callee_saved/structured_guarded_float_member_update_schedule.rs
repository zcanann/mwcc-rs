//! Scheduling for repeated guarded float-to-integer member updates.
//!
//! A boolean retained across a sequence of direction-state comparisons can be
//! set by either of two guarded updates to the same integer member. Build 163
//! keeps the boolean write beside the conversion setup, converts the loaded
//! integer in place, and assigns the two conversion images to the opposite
//! extraction lanes. This module owns that paired physical schedule after
//! register allocation.

#[allow(unused_imports)]
use super::*;

const CONVERSION_SCHEDULE: [usize; 14] = [0, 3, 4, 13, 1, 2, 5, 6, 7, 8, 9, 10, 11, 12];
const FIRST_COMPARISON_SCHEDULE: [usize; 7] = [3, 2, 4, 0, 5, 1, 6];
const LATER_COMPARISON_SCHEDULE: [usize; 4] = [1, 0, 2, 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConversionPacket {
    start: usize,
    member_base: u8,
    member_offset: i16,
    value: u8,
    boolean: u8,
    input_high: i16,
    output_high: i16,
}

impl Generator {
    pub(crate) fn schedule_guarded_float_member_updates(&mut self) {
        let Some([first, second]) = guarded_float_member_update_pair(&self.output.instructions)
        else {
            return;
        };
        let Some(comparisons) =
            direction_state_comparisons(&self.output, first.start + 14, first.member_base)
        else {
            return;
        };

        // The packets retain their length, so scheduling the earlier one cannot
        // move the later packet's start.
        self.schedule_guarded_float_member_update(first, second.output_high);
        self.schedule_guarded_float_member_update(second, first.output_high);
        self.apply_guarded_member_update_permutation(comparisons[0], &FIRST_COMPARISON_SCHEDULE);
        for start in &comparisons[1..] {
            self.apply_guarded_member_update_permutation(*start, &LATER_COMPARISON_SCHEDULE);
        }
        self.schedule_guarded_float_member_update_return(first.boolean, second.start + 14);
    }

    fn schedule_guarded_float_member_update(&mut self, packet: ConversionPacket, input_high: i16) {
        self.apply_guarded_member_update_permutation(packet.start, &CONVERSION_SCHEDULE);

        let window = &mut self.output.instructions[packet.start..packet.start + 14];
        let Instruction::XorImmediateShifted { a, .. } = &mut window[4] else {
            unreachable!("the integer conversion was recognized")
        };
        *a = packet.value;
        let Instruction::StoreWord { s, offset, .. } = &mut window[5] else {
            unreachable!("the conversion low-word store was recognized")
        };
        *s = packet.value;
        *offset = input_high + 4;
        let Instruction::StoreWord { offset, .. } = &mut window[6] else {
            unreachable!("the conversion high-word store was recognized")
        };
        *offset = input_high;
        let Instruction::LoadFloatDouble { offset, .. } = &mut window[7] else {
            unreachable!("the conversion image load was recognized")
        };
        *offset = input_high;
    }

    fn apply_guarded_member_update_permutation(&mut self, start: usize, schedule: &[usize]) {
        let mut current: Vec<usize> = (0..schedule.len()).collect();
        for (destination, &original) in schedule.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the guarded member update schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }

    fn schedule_guarded_float_member_update_return(&mut self, boolean: u8, after: usize) {
        let Some(call) = self.output.instructions[after..]
            .iter()
            .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|relative| after + relative)
        else {
            return;
        };
        if matches!(
            self.output.instructions.get(call + 1..call + 4),
            Some([
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::Or { a: 3, s, b },
                Instruction::LoadWord { d, a: 1, .. },
            ]) if *s == boolean && s == b && *d == boolean
        ) {
            self.move_instruction_before(call + 2, call + 1);
        }
    }
}

fn direction_state_comparisons(
    output: &mwcc_machine_code::MachineFunction,
    first: usize,
    member_base: u8,
) -> Option<[usize; 4]> {
    let second = first + 10;
    let third = second + 7;
    let fourth = third + 8;
    let [Instruction::LoadByteZero {
        d: first_saved,
        a: first_byte_base,
        offset: first_byte_offset,
    }, Instruction::LoadByteZero {
        d: second_saved,
        a: second_byte_base,
        offset: second_byte_offset,
    }, Instruction::LoadFloatSingle {
        d: 2,
        a: first_float_base,
        offset: first_float_offset,
    }, Instruction::LoadWord {
        d: 4,
        a: 0,
        offset: 0,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 4,
        offset: threshold_offset,
    }, Instruction::FloatNegate { d: 0, b: 0 }, Instruction::FloatCompareOrdered { a: 2, b: 0 }] =
        output.instructions.get(first..first + 7)?
    else {
        return None;
    };
    if *first_saved == *second_saved
        || *first_byte_base != member_base
        || *second_byte_base != member_base
        || *first_float_base != member_base
        || *second_byte_offset != *first_byte_offset + 1
    {
        return None;
    }
    let later = [
        (second, *first_float_offset, false),
        (third, first_float_offset.checked_add(4)?, true),
        (fourth, first_float_offset.checked_add(4)?, false),
    ];
    for (start, member_offset, negated) in later {
        let length = 4 + usize::from(negated);
        let window = output.instructions.get(start..start + length)?;
        let (
            Instruction::LoadFloatSingle {
                d: 2,
                a: later_base,
                offset: later_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 4,
                offset: later_threshold,
            },
        ) = (&window[0], &window[1], &window[2])
        else {
            return None;
        };
        if *later_base != member_base
            || *later_offset != member_offset
            || later_threshold != threshold_offset
            || (negated && !matches!(window[3], Instruction::FloatNegate { d: 0, b: 0 }))
            || !matches!(
                window[3 + usize::from(negated)],
                Instruction::FloatCompareOrdered { a: 2, b: 0 }
            )
            || !schedule_relocations::same_target_value(
                &output.relocations,
                &output.constants,
                first + 3,
                start + 1,
            )
        {
            return None;
        }
    }
    Some([first, second, third, fourth])
}

fn guarded_float_member_update_pair(instructions: &[Instruction]) -> Option<[ConversionPacket; 2]> {
    let packets: Vec<_> = instructions
        .windows(14)
        .enumerate()
        .filter_map(|(start, window)| conversion_packet(start, window))
        .collect();
    packets.windows(2).find_map(|pair| {
        let [first, second] = pair else {
            unreachable!("windows(2) always returns pairs")
        };
        (first.member_base == second.member_base
            && first.member_offset == second.member_offset
            && first.boolean == second.boolean
            && first.input_high == second.input_high
            && first.output_high != second.output_high
            && first.output_high.abs_diff(second.output_high) == 8)
            .then_some([*first, *second])
    })
}

fn conversion_packet(start: usize, window: &[Instruction]) -> Option<ConversionPacket> {
    let [Instruction::LoadWord {
        d: value,
        a: member_base,
        offset: member_offset,
    }, Instruction::XorImmediateShifted {
        a: 0,
        s: converted,
        immediate: 0x8000,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: low_offset,
    }, Instruction::AddImmediateShifted {
        d: 0,
        a: 0,
        immediate: 0x4330,
    }, Instruction::LoadFloatDouble {
        d: bias,
        a: 0,
        offset: 0,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: high_offset,
    }, Instruction::LoadFloatDouble {
        d: image,
        a: 1,
        offset: input_high,
    }, Instruction::FloatSubtractSingle {
        d: difference,
        a: image_source,
        b: bias_source,
    }, Instruction::FloatSubtractSingle {
        d: adjusted,
        a: difference_source,
        b: argument,
    }, Instruction::ConvertToIntegerWordZero {
        d: converted_float,
        b: adjusted_source,
    }, Instruction::StoreFloatDouble {
        s: stored_float,
        a: 1,
        offset: output_high,
    }, Instruction::LoadWord {
        d: result,
        a: 1,
        offset: output_low,
    }, Instruction::StoreWord {
        s: stored_result,
        a: stored_base,
        offset: stored_offset,
    }, Instruction::AddImmediate {
        d: boolean,
        a: 0,
        immediate: 1,
    }] = window
    else {
        return None;
    };
    (*value == *converted
        && *low_offset == *high_offset + 4
        && *high_offset == *input_high
        && *bias == *bias_source
        && *image == *image_source
        && *image == *difference
        && *difference == *difference_source
        && *adjusted == *adjusted_source
        && *argument == 1
        && *converted_float == *adjusted
        && *stored_float == *converted_float
        && *output_low == *output_high + 4
        && *result == *stored_result
        && *stored_base == *member_base
        && *stored_offset == *member_offset
        && (14..=31).contains(boolean))
    .then_some(ConversionPacket {
        start,
        member_base: *member_base,
        member_offset: *member_offset,
        value: *value,
        boolean: *boolean,
        input_high: *input_high,
        output_high: *output_high,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(output_high: i16) -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 8216,
            },
            Instruction::XorImmediateShifted {
                a: 0,
                s: 4,
                immediate: 0x8000,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 0x4330,
            },
            Instruction::LoadFloatDouble {
                d: 2,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 16,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 16,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: output_high,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: output_high + 4,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 8216,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 0,
                immediate: 1,
            },
        ]
    }

    #[test]
    fn recognizes_paired_updates_with_complementary_output_lanes() {
        let mut instructions = packet(16);
        instructions.push(Instruction::Branch { target: 29 });
        instructions.extend(packet(24));

        assert_eq!(
            guarded_float_member_update_pair(&instructions),
            Some([
                ConversionPacket {
                    start: 0,
                    member_base: 3,
                    member_offset: 8216,
                    value: 4,
                    boolean: 31,
                    input_high: 16,
                    output_high: 16,
                },
                ConversionPacket {
                    start: 15,
                    member_base: 3,
                    member_offset: 8216,
                    value: 4,
                    boolean: 31,
                    input_high: 16,
                    output_high: 24,
                },
            ])
        );
    }

    #[test]
    fn rejects_unrelated_conversion_destinations() {
        let mut instructions = packet(16);
        let mut second = packet(24);
        second[12] = Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 8220,
        };
        instructions.extend(second);

        assert_eq!(guarded_float_member_update_pair(&instructions), None);
    }
}
