//! Cross-statement scheduling for a frame-resident decimal digit pair.
//!
//! MWCC overlaps the two-digit arithmetic with the independent initialization
//! and range-test load that follow it. This owner acts on the complete selected
//! region, before allocation, so the overlapping byte values also receive the
//! measured volatile-register preferences.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_frame_digit_pair(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.schedule_latency_slots
        {
            return;
        }
        let Some((start, digit, most)) =
            self.output
                .instructions
                .windows(9)
                .enumerate()
                .find_map(|(start, window)| {
                    frame_digit_pair_registers(window).map(|v| (start, v.0, v.1))
                })
        else {
            return;
        };

        // The second digit must retain r4 while the earlier-issued range-test
        // byte occupies r5. These are preferences, not hard assignments:
        // overlapping live values and architectural constraints still win.
        self.prefer_virtual_general(digit, 4);
        self.prefer_virtual_general(most, 5);

        // Source selection emits:
        //   digit0; bias0; digit1; scale; add; bias1; most; false; compare
        // MWCC schedules:
        //   digit0; false; most; bias0; digit1; scale; compare; add; bias1
        self.move_frame_digit_instruction_before(start + 7, start + 1);
        self.move_frame_digit_instruction_before(start + 7, start + 2);
        self.move_frame_digit_instruction_before(start + 8, start + 6);
    }

    fn move_frame_digit_instruction_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let old_len = self.output.instructions.len();
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old == from {
                    to
                } else if (to..from).contains(&old) {
                    old + 1
                } else {
                    old
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn frame_digit_pair_registers(window: &[Instruction]) -> Option<(u8, u8)> {
    let [Instruction::LoadByteZero {
        d: room,
        a: 1,
        offset: first_offset,
    }, Instruction::AddImmediate {
        d: biased_room,
        a: room_source,
        immediate: first_bias,
    }, Instruction::LoadByteZero {
        d: digit,
        a: 1,
        offset: second_offset,
    }, Instruction::MultiplyImmediate {
        d: scaled_room,
        a: scale_source,
        immediate: radix,
    }, Instruction::Add {
        d: combined_room,
        a: digit_source,
        b: add_room_source,
    }, Instruction::AddImmediate {
        d: final_room,
        a: combined_source,
        immediate: second_bias,
    }, Instruction::LoadByteZero {
        d: most,
        a: 1,
        offset: most_offset,
    }, Instruction::AddImmediate {
        d: boolean,
        a: 0,
        immediate: 0,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared,
        immediate: lower_bound,
    }] = window
    else {
        return None;
    };

    (*biased_room == *room
        && *room_source == *room
        && *scaled_room == *room
        && *scale_source == *room
        && *combined_room == *room
        && *digit_source == *digit
        && *add_room_source == *room
        && *final_room == *room
        && *combined_source == *room
        && *first_bias == *second_bias
        && *first_bias < 0
        && *radix > 1
        && i32::from(*first_offset) == i32::from(*most_offset) + 2
        && i32::from(*second_offset) == i32::from(*most_offset) + 3
        && *compared == *most
        && *lower_bound != 0
        && *room != *digit
        && *room != *most
        && *digit != *most
        && *boolean != *room
        && *boolean != *digit
        && *boolean != *most)
        .then_some((*digit, *most))
}
