//! Physical scheduling for two display-list packets before a mixed call.
//!
//! The source expands two adjacent eight-byte packets immediately before a
//! call with integer, stack, and converted-float arguments. Selection emits a
//! dependency-correct serial order. MWCC overlaps packet construction with the
//! conversion chains; this pass recognizes the complete region before applying
//! that measured schedule and its scratch lanes.

#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 45] = [
    28, 0, 11, 2, 1, 3, 14, 5, 4, 6, 15, 7, 27, 35, 29, 8, 12, 30, 40, 9, 41, 10, 25, 43, 42, 13,
    17, 19, 23, 16, 22, 24, 26, 44, 31, 20, 32, 21, 33, 36, 34, 18, 37, 38, 39,
];

impl Generator {
    pub(crate) fn schedule_structured_frame_packet_call(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(46)
            .position(is_serial_packet_call)
        else {
            return;
        };

        let mut current: Vec<usize> = (0..SCHEDULE.len()).collect();
        for (destination, &original) in SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the packet-call schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        assign_mwcc_packet_call_lanes(&mut self.output.instructions[start..start + SCHEDULE.len()]);
    }
}

fn is_serial_packet_call(window: &[Instruction]) -> bool {
    if window.len() != 46 {
        return false;
    }
    let first_packet = matches!(
        &window[..8],
        [
            Instruction::AddImmediate { immediate: 8, .. },
            Instruction::StoreWord {
                a: 1,
                offset: 20,
                ..
            },
            Instruction::AddImmediateShifted {
                a: 0,
                immediate: -4352,
                ..
            },
            Instruction::AddImmediate {
                immediate: 3312,
                ..
            },
            Instruction::StoreWord { offset: 0, .. },
            Instruction::AddImmediateShifted {
                a: 0,
                immediate: 3850,
                ..
            },
            Instruction::AddImmediate {
                immediate: 16388,
                ..
            },
            Instruction::StoreWord { offset: 4, .. },
        ]
    );
    let second_packet = matches!(
        &window[8..17],
        [
            Instruction::LoadWord {
                a: 1,
                offset: 20,
                ..
            },
            Instruction::AddImmediate { immediate: 8, .. },
            Instruction::StoreWord {
                a: 1,
                offset: 20,
                ..
            },
            Instruction::AddImmediateShifted {
                a: 0,
                immediate: -768,
                ..
            },
            Instruction::AddImmediate { immediate: -1, .. },
            Instruction::StoreWord { offset: 0, .. },
            Instruction::AddImmediateShifted {
                a: 0,
                immediate: -3,
                ..
            },
            Instruction::AddImmediate {
                immediate: -898,
                ..
            },
            Instruction::StoreWord { offset: 4, .. },
        ]
    );
    let arguments = matches!(
        &window[17..],
        [
            Instruction::AddImmediate { d: 3, a: 1, .. },
            Instruction::LoadWord {
                d: 4,
                a: 26,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadHalfwordZero {
                d: 6,
                a: 26,
                offset: 4,
            },
            Instruction::LoadHalfwordZero {
                d: 7,
                a: 26,
                offset: 6,
            },
            Instruction::AddImmediate {
                d: 8,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 0,
                immediate: 2,
            },
            Instruction::AddImmediate {
                d: 10,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 11,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                a: 1,
                offset: 8,
                ..
            },
            Instruction::XorImmediateShifted {
                a: 15,
                s: 15,
                immediate: 32768,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 17200,
            },
            Instruction::AddImmediateShifted {
                d: 11,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadFloatDouble {
                d: 2,
                a: 11,
                offset: 0,
            },
            Instruction::StoreWord {
                a: 1,
                offset: 28,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 24,
                ..
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 24,
            },
            Instruction::FloatSubtractSingle { d: 1, a: 0, b: 2 },
            Instruction::XorImmediateShifted {
                a: 14,
                s: 14,
                immediate: 32768,
            },
            Instruction::StoreWord {
                a: 1,
                offset: 36,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 32,
                ..
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 32,
            },
            Instruction::FloatSubtractSingle { d: 2, a: 0, b: 2 },
            Instruction::AddImmediateShifted {
                d: 11,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadFloatSingle {
                d: 3,
                a: 11,
                offset: 0,
            },
            Instruction::FloatMove { d: 4, b: 3 },
            Instruction::AddImmediate {
                a: 0,
                immediate: 11,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 12,
                ..
            },
            Instruction::BranchAndLink { .. },
        ]
    );

    first_packet
        && second_packet
        && arguments
        && packet_dependencies(window)
        && constant_word_dependencies(window, 2, 3, 4)
        && constant_word_dependencies(window, 5, 6, 7)
        && constant_word_dependencies(window, 11, 12, 13)
        && constant_word_dependencies(window, 14, 15, 16)
}

fn constant_word_dependencies(
    window: &[Instruction],
    high: usize,
    low: usize,
    store: usize,
) -> bool {
    matches!(
        (&window[high], &window[low], &window[store]),
        (
            Instruction::AddImmediateShifted { d: high_value, .. },
            Instruction::AddImmediate {
                d: word,
                a: low_base,
                ..
            },
            Instruction::StoreWord { s: stored, .. },
        ) if high_value == low_base && word == stored
    )
}

fn packet_dependencies(window: &[Instruction]) -> bool {
    let (
        Instruction::AddImmediate {
            d: first_advanced,
            a: first_cursor,
            ..
        },
        Instruction::StoreWord {
            s: first_published, ..
        },
        Instruction::StoreWord {
            a: first_word_base, ..
        },
        Instruction::StoreWord {
            a: first_second_base,
            ..
        },
        Instruction::LoadWord {
            d: second_cursor, ..
        },
        Instruction::AddImmediate {
            d: second_advanced,
            a: second_bump_source,
            ..
        },
        Instruction::StoreWord {
            s: second_published,
            ..
        },
        Instruction::StoreWord {
            a: second_word_base,
            ..
        },
        Instruction::StoreWord {
            a: second_second_base,
            ..
        },
    ) = (
        &window[0],
        &window[1],
        &window[4],
        &window[7],
        &window[8],
        &window[9],
        &window[10],
        &window[13],
        &window[16],
    )
    else {
        return false;
    };
    first_advanced == first_published
        && first_cursor == first_word_base
        && first_word_base == first_second_base
        && second_cursor == second_bump_source
        && second_advanced == second_published
        && second_cursor == second_word_base
        && second_word_base == second_second_base
}

fn assign_mwcc_packet_call_lanes(window: &mut [Instruction]) {
    window[0] = Instruction::load_immediate_shifted(11, 17200);
    window[1] = Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 8,
    };
    window[2] = Instruction::load_immediate_shifted(7, -768);
    window[3] = Instruction::load_immediate_shifted(4, -4352);
    window[4] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 20,
    };
    window[5] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: 3312,
    };
    window[6] = Instruction::load_immediate_shifted(6, -3);
    window[7] = Instruction::load_immediate_shifted(4, 3850);
    window[8] = Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: 0,
    };
    window[9] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: 16388,
    };
    window[10] = Instruction::AddImmediate {
        d: 8,
        a: 6,
        immediate: -898,
    };
    window[11] = Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: 4,
    };
    window[12] = Instruction::XorImmediateShifted {
        a: 4,
        s: 15,
        immediate: 32768,
    };
    window[13] = Instruction::XorImmediateShifted {
        a: 0,
        s: 14,
        immediate: 32768,
    };
    window[14] = Instruction::load_immediate_shifted(5, 0);
    window[15] = Instruction::LoadWord {
        d: 10,
        a: 1,
        offset: 20,
    };
    window[16] = Instruction::AddImmediate {
        d: 9,
        a: 7,
        immediate: -1,
    };
    window[17] = Instruction::LoadFloatDouble {
        d: 2,
        a: 5,
        offset: 0,
    };
    window[18] = Instruction::load_immediate_shifted(3, 0);
    window[19] = Instruction::AddImmediate {
        d: 7,
        a: 10,
        immediate: 8,
    };
    window[20] = Instruction::LoadFloatSingle {
        d: 3,
        a: 3,
        offset: 0,
    };
    window[21] = Instruction::StoreWord {
        s: 7,
        a: 1,
        offset: 20,
    };
    window[22] = Instruction::load_immediate(7, 0);
    window[23] = Instruction::load_immediate(6, 11);
    window[24] = Instruction::FloatMove { d: 4, b: 3 };
    window[25] = Instruction::StoreWord {
        s: 9,
        a: 10,
        offset: 0,
    };
    window[26] = Instruction::AddImmediate {
        d: 3,
        a: 1,
        immediate: 20,
    };
    window[27] = Instruction::load_immediate(5, 0);
    window[28] = Instruction::load_immediate(9, 2);
    window[29] = Instruction::StoreWord {
        s: 8,
        a: 10,
        offset: 4,
    };
    window[30] = Instruction::load_immediate(8, 0);
    window[31] = Instruction::load_immediate(10, 0);
    window[32] = Instruction::StoreWord {
        s: 7,
        a: 1,
        offset: 8,
    };
    window[33] = Instruction::StoreWord {
        s: 6,
        a: 1,
        offset: 12,
    };
    window[34] = Instruction::StoreWord {
        s: 4,
        a: 1,
        offset: 28,
    };
    window[35] = Instruction::LoadHalfwordZero {
        d: 6,
        a: 26,
        offset: 4,
    };
    window[36] = Instruction::StoreWord {
        s: 11,
        a: 1,
        offset: 24,
    };
    window[37] = Instruction::LoadHalfwordZero {
        d: 7,
        a: 26,
        offset: 6,
    };
    window[38] = Instruction::LoadFloatDouble {
        d: 0,
        a: 1,
        offset: 24,
    };
    window[39] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 36,
    };
    window[40] = Instruction::FloatSubtractSingle { d: 1, a: 0, b: 2 };
    window[41] = Instruction::LoadWord {
        d: 4,
        a: 26,
        offset: 20,
    };
    window[42] = Instruction::StoreWord {
        s: 11,
        a: 1,
        offset: 32,
    };
    window[43] = Instruction::LoadFloatDouble {
        d: 0,
        a: 1,
        offset: 32,
    };
    window[44] = Instruction::FloatSubtractSingle { d: 2, a: 0, b: 2 };
}

#[cfg(test)]
#[path = "structured_frame_packet_call_schedule_tests.rs"]
mod tests;
