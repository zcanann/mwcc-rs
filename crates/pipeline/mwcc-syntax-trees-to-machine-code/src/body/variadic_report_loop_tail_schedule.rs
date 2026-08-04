//! Schedule independent cursor and counter updates after variadic loop calls.
//!
//! A source loop exposes only its counter step; member addressing introduces a
//! second, optimizer-owned cursor. Build 163 advances that cursor before the
//! counter consumed by the loop comparison. Keep this late and physical so the
//! lowering stages retain semantic ownership of both induction values.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_variadic_report_loop_tails(&mut self) {
        if !self.behavior.schedule_latency_slots {
            return;
        }
        let mut search_from = 0;
        while let Some(relative) = self.output.instructions[search_from..]
            .windows(5)
            .enumerate()
            .find_map(|(relative, window)| {
                is_counter_before_cursor_report_tail(window, search_from + relative)
                    .then_some(relative)
            })
        {
            let start = search_from + relative;
            // This is a basic-block content schedule. Any incoming label stays
            // at the first tail slot while the two independent updates trade
            // positions; neither instruction owns a relocation or data patch.
            self.output.instructions.swap(start + 1, start + 2);
            search_from = start + 5;
        }
    }
}

fn is_counter_before_cursor_report_tail(window: &[Instruction], start: usize) -> bool {
    matches!(
        window,
        [
            Instruction::BranchAndLink { target },
            Instruction::AddImmediate {
                d: counter_destination,
                a: counter_source,
                immediate: counter_step,
            },
            Instruction::AddImmediate {
                d: cursor_destination,
                a: cursor_source,
                immediate: cursor_step,
            },
            Instruction::CompareLogicalWordImmediate { a: compared, .. },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_entry,
            },
        ] if target == "OSReport"
            && counter_destination == counter_source
            && cursor_destination == cursor_source
            && counter_destination != cursor_destination
            && compared == counter_destination
            && (14..=31).contains(counter_destination)
            && (14..=31).contains(cursor_destination)
            && matches!(counter_step, 1 | 2)
            && matches!(cursor_step, 4 | 16)
            && loop_entry < &start
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_report_loop_counter_before_word_cursor() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
            Instruction::AddImmediate {
                d: 25,
                a: 25,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 27,
                a: 27,
                immediate: 4,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 25,
                immediate: 16,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 0,
            },
        ];
        assert!(is_counter_before_cursor_report_tail(&instructions, 1));
    }

    #[test]
    fn rejects_a_tail_whose_comparison_consumes_the_cursor() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
            Instruction::AddImmediate {
                d: 25,
                a: 25,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 27,
                a: 27,
                immediate: 4,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 27,
                immediate: 16,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 0,
            },
        ];
        assert!(!is_counter_before_cursor_report_tail(&instructions, 1));
    }
}
