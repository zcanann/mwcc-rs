//! Preloop scheduling for retained assertion string high halves.
//!
//! MWCC fills the list-base load latency with both `lis` instructions before it
//! loads the iterator begin sentinel. Selection emits the same values in source
//! evaluation order; this pass applies the measured dependency-safe schedule.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_loop_assertion_string_highs(&mut self) {
        let [(_, first_high), (_, second_high)] = self.loop_assertion_string_highs.as_slice() else {
            return;
        };
        let Some(start) =
            string_high_schedule_start(&self.output.instructions, *first_high, *second_high)
        else {
            return;
        };
        self.move_instruction_before(start + 1, start);
        self.move_instruction_before(start + 2, start + 1);
    }
}

fn string_high_schedule_start(
    instructions: &[Instruction],
    first_high: u8,
    second_high: u8,
) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    a: base,
                    offset: 16,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: first,
                    a: 0,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: second,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    a: end_base,
                    immediate: 16,
                    ..
                },
            ] if first == &first_high && second == &second_high && base == end_base
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(first: u8, second: u8) -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 16,
            },
            Instruction::AddImmediateShifted {
                d: first,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: second,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 3,
                immediate: 16,
            },
        ]
    }

    #[test]
    fn recognizes_the_begin_high_high_end_region() {
        assert_eq!(string_high_schedule_start(&stream(28, 29), 28, 29), Some(0));
    }

    #[test]
    fn rejects_unplanned_string_homes() {
        assert_eq!(string_high_schedule_start(&stream(4, 5), 28, 29), None);
    }
}
