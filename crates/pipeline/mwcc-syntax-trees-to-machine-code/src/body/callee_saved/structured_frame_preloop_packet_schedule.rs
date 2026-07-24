//! Scheduling for three frame-cursor packets immediately before a loop.
//!
//! MWCC overlaps the packets' constant construction and cursor publication,
//! then issues the loop divisor setup. This pass owns only that packet
//! permutation. The width/quotient lanes remain compatible with the following
//! clamp diamond; recoloring that larger live range belongs to a later pass.

#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 28] = [
    0, 2, 5, 1, 3, 11, 6, 4, 12, 14, 19, 7, 21, 24, 27, 8, 9, 10, 13, 15, 16, 17, 18, 20, 22, 23,
    25, 26,
];

impl Generator {
    pub(crate) fn schedule_structured_frame_preloop_packets(&mut self) {
        let Some(region) =
            self.output
                .instructions
                .windows(29)
                .enumerate()
                .find_map(|(start, window)| {
                    serial_preloop_packet_lanes(window)
                        .map(|lanes| PreloopPacketRegion { start, lanes })
                })
        else {
            return;
        };
        let start = region.start;

        let mut current: Vec<usize> = (0..SCHEDULE.len()).collect();
        for (destination, &original) in SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the pre-loop packet schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        assign_packet_lanes(
            &mut self.output.instructions[start..start + SCHEDULE.len()],
            region.lanes,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreloopPacketRegion {
    start: usize,
    lanes: DivisorLanes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DivisorLanes {
    width: u8,
    quotient: u8,
}

fn serial_preloop_packet_lanes(window: &[Instruction]) -> Option<DivisorLanes> {
    (window.len() == 29
        && matches!(
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
                    immediate: -4336,
                    ..
                },
                Instruction::AddImmediate {
                    immediate: 3312,
                    ..
                },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::AddImmediateShifted {
                    a: 0,
                    immediate: 3866,
                    ..
                },
                Instruction::AddImmediate {
                    immediate: 29004,
                    ..
                },
                Instruction::StoreWord { offset: 4, .. },
            ]
        )
        && matches!(
            &window[8..16],
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
                Instruction::AddImmediate {
                    immediate: -7169,
                    ..
                },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::AddImmediate {
                    a: 0,
                    immediate: -1480,
                    ..
                },
                Instruction::StoreWord { offset: 4, .. },
            ]
        )
        && matches!(
            &window[16..23],
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
                    immediate: -1280,
                    ..
                },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::AddImmediate {
                    a: 0,
                    immediate: -224,
                    ..
                },
                Instruction::StoreWord { offset: 4, .. },
            ]
        )
        && matches!(
            &window[23..],
            [
                Instruction::LoadHalfwordZero {
                    a: 26,
                    offset: 4,
                    ..
                },
                Instruction::AddImmediate {
                    a: 0,
                    immediate: 4096,
                    ..
                },
                Instruction::RotateAndMask {
                    shift: 1,
                    begin: 15,
                    end: 30,
                    ..
                },
                Instruction::DivideWordUnsigned { .. },
                Instruction::CompareWordImmediate {
                    a: 15,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward { .. },
            ]
        )
        && cursor_dependencies(window)
        && word_dependencies(window, 2, 3, 4)
        && word_dependencies(window, 5, 6, 7)
        && word_dependencies(window, 11, 12, 13))
    .then(|| divisor_lanes(window))
    .flatten()
}

fn cursor_dependencies(window: &[Instruction]) -> bool {
    cursor_packet(window, 0, None, 1, &[4, 7])
        && cursor_packet(window, 9, Some(8), 10, &[13, 15])
        && cursor_packet(window, 17, Some(16), 18, &[20, 22])
}

fn cursor_packet(
    window: &[Instruction],
    bump: usize,
    load: Option<usize>,
    publish: usize,
    stores: &[usize],
) -> bool {
    let Instruction::AddImmediate {
        d: advanced,
        a: cursor,
        ..
    } = window[bump]
    else {
        return false;
    };
    if let Some(load) = load {
        if !matches!(window[load], Instruction::LoadWord { d, .. } if d == cursor) {
            return false;
        }
    }
    matches!(window[publish], Instruction::StoreWord { s, .. } if s == advanced)
        && stores
            .iter()
            .all(|&store| matches!(window[store], Instruction::StoreWord { a, .. } if a == cursor))
}

fn word_dependencies(window: &[Instruction], high: usize, low: usize, store: usize) -> bool {
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

fn divisor_lanes(window: &[Instruction]) -> Option<DivisorLanes> {
    let [Instruction::LoadHalfwordZero { d: width, .. }, Instruction::AddImmediate {
        d: dividend,
        a: 0,
        immediate: 4096,
    }, Instruction::RotateAndMask {
        a: divisor,
        s: scaled_width,
        shift: 1,
        begin: 15,
        end: 30,
    }, Instruction::DivideWordUnsigned {
        d: quotient,
        a: divide_dividend,
        b: divide_divisor,
    }] = &window[23..27]
    else {
        return None;
    };
    (width == scaled_width
        && dividend == divide_dividend
        && divisor == divide_divisor
        && width != quotient)
        .then_some(DivisorLanes {
            width: *width,
            quotient: *quotient,
        })
}

fn assign_packet_lanes(window: &mut [Instruction], lanes: DivisorLanes) {
    window[0] = Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 8,
    };
    window[1] = Instruction::load_immediate_shifted(4, -4336);
    window[2] = Instruction::load_immediate_shifted(5, 3866);
    window[3] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 20,
    };
    window[4] = Instruction::AddImmediate {
        d: 6,
        a: 4,
        immediate: 3312,
    };
    window[5] = Instruction::load_immediate_shifted(4, -768);
    window[6] = Instruction::AddImmediate {
        d: 0,
        a: 5,
        immediate: 29004,
    };
    window[7] = Instruction::StoreWord {
        s: 6,
        a: 3,
        offset: 0,
    };
    window[8] = Instruction::AddImmediate {
        d: 6,
        a: 4,
        immediate: -7169,
    };
    window[9] = Instruction::load_immediate(5, -1480);
    window[10] = Instruction::load_immediate_shifted(4, -1280);
    window[11] = Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: 4,
    };
    window[12] = Instruction::load_immediate(0, -224);
    window[13] = Instruction::load_immediate(3, 4096);
    window[14] = Instruction::CompareWordImmediate {
        a: 15,
        immediate: 0,
    };
    window[15] = Instruction::LoadWord {
        d: 8,
        a: 1,
        offset: 20,
    };
    window[16] = Instruction::AddImmediate {
        d: 7,
        a: 8,
        immediate: 8,
    };
    window[17] = Instruction::StoreWord {
        s: 7,
        a: 1,
        offset: 20,
    };
    window[18] = Instruction::StoreWord {
        s: 6,
        a: 8,
        offset: 0,
    };
    window[19] = Instruction::StoreWord {
        s: 5,
        a: 8,
        offset: 4,
    };
    window[20] = Instruction::LoadWord {
        d: 6,
        a: 1,
        offset: 20,
    };
    window[21] = Instruction::AddImmediate {
        d: 5,
        a: 6,
        immediate: 8,
    };
    window[22] = Instruction::StoreWord {
        s: 5,
        a: 1,
        offset: 20,
    };
    window[23] = Instruction::StoreWord {
        s: 4,
        a: 6,
        offset: 0,
    };
    window[24] = Instruction::StoreWord {
        s: 0,
        a: 6,
        offset: 4,
    };
    window[25] = Instruction::LoadHalfwordZero {
        d: lanes.width,
        a: 26,
        offset: 4,
    };
    window[26] = Instruction::RotateAndMask {
        a: 0,
        s: lanes.width,
        shift: 1,
        begin: 15,
        end: 30,
    };
    window[27] = Instruction::DivideWordUnsigned {
        d: lanes.quotient,
        a: 3,
        b: 0,
    };
}

#[cfg(test)]
#[path = "structured_frame_preloop_packet_schedule_tests.rs"]
mod tests;
