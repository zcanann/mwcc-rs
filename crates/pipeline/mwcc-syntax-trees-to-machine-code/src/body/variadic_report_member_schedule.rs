//! Fill a split string-address dependency with saved-member report arguments.
//!
//! For `OSReport(format, object->byte, object->word)`, both member loads are
//! independent of the format address. MWCC places them between `lis` and
//! `addi`, retaining the saved object base and hiding the split-address gap.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_variadic_report_member_arguments(&mut self) {
        if let Some(start) = self
            .output
            .instructions
            .windows(6)
            .position(is_unscheduled_report)
            .filter(|start| {
                schedule_relocations::same_target_value(
                    &self.output.relocations,
                    &self.output.constants,
                    *start,
                    *start + 1,
                )
            })
        {
            self.move_report_instruction_before(start + 2, start + 1);
            self.move_report_instruction_before(start + 3, start + 2);
        }

        while let Some((start, index)) = duplicate_word_pair_report(&self.output.instructions) {
            // Selection order:
            //   format; index; load A; reload A; index+16; load B; reload B; crclr
            // Build 163 hides both load latencies and retains each loaded value:
            //   load B; index; load A; format; crclr; copy A; copy B; index+16
            basic_block_schedule::permute_contents(
                &mut self.output,
                start,
                [5, 1, 2, 0, 7, 3, 6, 4, 8],
            );
            self.output.instructions[start + 1] = Instruction::move_register(4, index);
            self.output.instructions[start + 5] = Instruction::move_register(6, 5);
            self.output.instructions[start + 6] = Instruction::move_register(9, 8);
        }

        while let Some(start) = two_word_member_report(&self.output.instructions) {
            // Selection order computes the saved format address first. Build
            // 163 starts the first independent member load, then fills its
            // latency with the format address before loading the second word.
            basic_block_schedule::permute_contents(&mut self.output, start, [1, 0, 2, 3, 4]);
        }

        while let Some((start, index)) = indexed_word_pair_report(&self.output.instructions) {
            // The two member loads are independent of both saved-index
            // arguments. Start each load early and retain the copied index in
            // the encoding MWCC selects after physical allocation.
            basic_block_schedule::permute_contents(
                &mut self.output,
                start,
                [2, 1, 4, 0, 3, 5, 6],
            );
            self.output.instructions[start + 1] = Instruction::move_register(4, index);
        }
    }

    fn move_report_instruction_before(&mut self, from: usize, to: usize) {
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => {
                    *target = if *target == from {
                        to
                    } else if (to..from).contains(&*target) {
                        *target + 1
                    } else {
                        *target
                    };
                }
                _ => {}
            }
        }
    }

}

fn two_word_member_report(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: format_base,
                    ..
                },
                Instruction::LoadWord {
                    d: 4,
                    a: first_base,
                    offset: first_offset,
                },
                Instruction::LoadWord {
                    d: 5,
                    a: second_base,
                    offset: second_offset,
                },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] if (14..=31).contains(format_base)
                && (14..=31).contains(first_base)
                && first_base == second_base
                && first_offset != second_offset
                && target == "OSReport"
        )
    })
}

fn indexed_word_pair_report(instructions: &[Instruction]) -> Option<(usize, u8)> {
    instructions.windows(7).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediate {
                d: 3,
                a: format_base,
                ..
            },
            Instruction::AddImmediate {
                d: 4,
                a: first_index,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: first_base,
                offset: first_offset,
            },
            Instruction::AddImmediate {
                d: 6,
                a: second_index,
                immediate: 4,
            },
            Instruction::LoadWord {
                d: 7,
                a: second_base,
                offset: second_offset,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] = window
        else {
            return None;
        };
        ((14..=31).contains(format_base)
            && (14..=31).contains(first_index)
            && first_index == second_index
            && (14..=31).contains(first_base)
            && first_base == second_base
            && second_offset.checked_sub(*first_offset) == Some(16)
            && target == "OSReport")
            .then_some((start, *first_index))
    })
}

fn duplicate_word_pair_report(instructions: &[Instruction]) -> Option<(usize, u8)> {
    instructions.windows(9).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediate { d: 3, a: format_base, .. },
            Instruction::AddImmediate {
                d: 4,
                a: first_index,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: first_base,
                offset: first_offset,
            },
            Instruction::LoadWord {
                d: 6,
                a: duplicate_first_base,
                offset: duplicate_first_offset,
            },
            Instruction::AddImmediate {
                d: 7,
                a: second_index,
                immediate: 16,
            },
            Instruction::LoadWord {
                d: 8,
                a: second_base,
                offset: second_offset,
            },
            Instruction::LoadWord {
                d: 9,
                a: duplicate_second_base,
                offset: duplicate_second_offset,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] = window else {
            return None;
        };
        ((14..=31).contains(format_base)
            && (14..=31).contains(first_index)
            && first_index == second_index
            && first_base == duplicate_first_base
            && first_offset == duplicate_first_offset
            && second_base == duplicate_second_base
            && second_offset == duplicate_second_offset
            && first_base == second_base
            && second_offset.checked_sub(*first_offset) == Some(64)
            && target == "OSReport")
            .then_some((start, *first_index))
    })
}

fn is_unscheduled_report(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::AddImmediateShifted { d: 3, a: 0, .. },
        Instruction::AddImmediate { d: 3, a: 3, .. },
        Instruction::LoadByteZero { d: 4, a: byte_base, .. },
        Instruction::LoadWord { d: 5, a: word_base, .. },
        Instruction::ConditionRegisterClear { d: 6 },
        Instruction::BranchAndLink { target },
    ] if byte_base == word_base && (14..=31).contains(byte_base) && target == "OSReport")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_saved_member_variadic_report_arguments() {
        let instructions = [
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::LoadByteZero { d: 4, a: 31, offset: 12 },
            Instruction::LoadWord { d: 5, a: 31, offset: 16 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "OSReport".into() },
        ];
        assert!(is_unscheduled_report(&instructions));
    }

    #[test]
    fn recognizes_two_duplicated_word_arguments_from_one_indexed_context() {
        let instructions = [
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 68,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 25,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: 27,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 6,
                a: 27,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 7,
                a: 25,
                immediate: 16,
            },
            Instruction::LoadWord {
                d: 8,
                a: 27,
                offset: 64,
            },
            Instruction::LoadWord {
                d: 9,
                a: 27,
                offset: 64,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
        ];
        assert_eq!(duplicate_word_pair_report(&instructions), Some((0, 25)));
    }

    #[test]
    fn duplicate_word_report_schedule_preserves_the_loop_entry_boundary() {
        let mut output = mwcc_machine_code::MachineFunction::new("report_loop");
        output.instructions = vec![
            Instruction::Branch { target: 1 },
            Instruction::Branch { target: 2 },
            Instruction::Branch { target: 3 },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 68,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 25,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: 27,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 6,
                a: 27,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 7,
                a: 25,
                immediate: 16,
            },
            Instruction::LoadWord {
                d: 8,
                a: 27,
                offset: 64,
            },
            Instruction::LoadWord {
                d: 9,
                a: 27,
                offset: 64,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 3,
            },
        ];

        basic_block_schedule::permute_contents(
            &mut output,
            3,
            [5, 1, 2, 0, 7, 3, 6, 4, 8],
        );

        assert!(matches!(
            output.instructions[3],
            Instruction::LoadWord {
                d: 8,
                a: 27,
                offset: 64
            }
        ));
        assert_eq!(output.instructions[2], Instruction::Branch { target: 3 });
        assert!(matches!(
            output.instructions[12],
            Instruction::BranchConditionalForward { target: 3, .. }
        ));
    }

    #[test]
    fn recognizes_two_word_members_after_a_saved_format_address() {
        let instructions = [
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 116,
            },
            Instruction::LoadWord {
                d: 4,
                a: 28,
                offset: 132,
            },
            Instruction::LoadWord {
                d: 5,
                a: 28,
                offset: 128,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
        ];
        assert_eq!(two_word_member_report(&instructions), Some(0));
    }

    #[test]
    fn recognizes_indexed_word_pair_report_arguments() {
        let instructions = [
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 232,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 25,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: 27,
                offset: 420,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 25,
                immediate: 4,
            },
            Instruction::LoadWord {
                d: 7,
                a: 27,
                offset: 436,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "OSReport".into(),
            },
        ];
        assert_eq!(indexed_word_pair_report(&instructions), Some((0, 25)));
    }
}
