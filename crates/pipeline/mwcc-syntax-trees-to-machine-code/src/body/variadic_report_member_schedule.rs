//! Fill a split string-address dependency with saved-member report arguments.
//!
//! For `OSReport(format, object->byte, object->word)`, both member loads are
//! independent of the format address. MWCC places them between `lis` and
//! `addi`, retaining the saved object base and hiding the split-address gap.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_variadic_report_member_arguments(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(6)
            .position(is_unscheduled_report)
        else {
            return;
        };
        if !schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start,
            start + 1,
        ) {
            return;
        }

        self.move_report_instruction_before(start + 2, start + 1);
        self.move_report_instruction_before(start + 3, start + 2);
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
}
