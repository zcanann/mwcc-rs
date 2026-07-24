//! Build-163 register schedule at a two-group floating-point `||` boundary.
//!
//! MWCC retains each group's value operands but rematerializes the shared zero
//! literal when control advances to the second group. Keeping this policy out
//! of the general condition cache preserves its dominance rules and confines
//! the physical register permutation to the complete measured shape.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_float_or_groups(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(20)
            .position(is_coalesced_float_or_groups)
        else {
            return;
        };
        let zero_index = start + 1;
        let insertion = start + 11;
        let zero_relocations: Vec<_> = self
            .output
            .relocations
            .iter()
            .filter(|relocation| relocation.instruction_index == zero_index)
            .cloned()
            .collect();
        if zero_relocations.is_empty() {
            return;
        }

        let zero_load = self.output.instructions[zero_index].clone();
        self.output.instructions.insert(insertion, zero_load);
        self.labels.inserted(insertion, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= insertion {
                relocation.instruction_index += 1;
            }
        }
        for mut relocation in zero_relocations {
            relocation.instruction_index = insertion;
            self.output.relocations.push(relocation);
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > insertion =>
                {
                    *target += 1;
                }
                _ => {}
            }
        }

        // MWCC's two persistent operands occupy f2 (member value) and f1
        // (zero), leaving f0 for the stack value and destructive sums.
        for (index, destination) in [(start, 2), (start + 1, 1), (insertion, 1)] {
            match &mut self.output.instructions[index] {
                Instruction::LoadFloatSingle { d, .. } => *d = destination,
                _ => unreachable!(),
            }
        }
        for index in [start + 2, start + 12] {
            match &mut self.output.instructions[index] {
                Instruction::FloatCompareOrdered { a, b } => {
                    *a = 2;
                    *b = 1;
                }
                _ => unreachable!(),
            }
        }
        for index in [start + 4, start + 14] {
            match &mut self.output.instructions[index] {
                Instruction::LoadFloatSingle { d, .. } => *d = 0,
                _ => unreachable!(),
            }
        }
        for index in [start + 5, start + 8, start + 15, start + 18] {
            match &mut self.output.instructions[index] {
                Instruction::FloatCompareOrdered { a, b } => {
                    *a = 0;
                    *b = 1;
                }
                _ => unreachable!(),
            }
        }
        for index in [start + 7, start + 17] {
            match &mut self.output.instructions[index] {
                Instruction::FloatAddSingle { d, a, b } => {
                    *d = 0;
                    *a = 0;
                    *b = 2;
                }
                _ => unreachable!(),
            }
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn is_coalesced_float_or_groups(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: 1, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: second_group_a, .. },
        Instruction::LoadFloatSingle { d: 2, a: stack_a, offset: stack_offset_a },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward { target: second_group_b, .. },
        Instruction::FloatAddSingle { d: 2, a: 2, b: 1 },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::ConditionRegisterOr { .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: exit_a, .. },
        Instruction::LoadFloatSingle { d: 2, a: stack_b, offset: stack_offset_b },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward { target: exit_b, .. },
        Instruction::FloatAddSingle { d: 1, a: 2, b: 1 },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::ConditionRegisterOr { .. },
        Instruction::BranchConditionalForward { .. },
    ] if second_group_a == second_group_b
        && stack_a == stack_b
        && stack_offset_a == stack_offset_b
        && exit_a == exit_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_two_coalesced_float_or_groups() {
        let branch = |target| Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target,
        };
        let instructions = [
            Instruction::LoadFloatSingle { d: 1, a: 31, offset: 252 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            branch(11),
            Instruction::LoadFloatSingle { d: 2, a: 1, offset: 24 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            branch(11),
            Instruction::FloatAddSingle { d: 2, a: 2, b: 1 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            branch(21),
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            branch(21),
            Instruction::LoadFloatSingle { d: 2, a: 1, offset: 24 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            branch(21),
            Instruction::FloatAddSingle { d: 1, a: 2, b: 1 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 0, b: 2 },
            branch(20),
        ];
        assert!(is_coalesced_float_or_groups(&instructions));
    }
}
