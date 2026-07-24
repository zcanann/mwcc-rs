//! Final lifetime scheduling for a symmetric clamp over a two-value sum.
//!
//! Structured arm lowering independently reloads both addends in the lower
//! comparison and reloads one again for each assignment. MWCC retains the
//! pre-branch sum and first addend across the diamond, materializing the
//! negated bound once in the lower arm.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_symmetric_sum_clamp(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(19)
            .position(is_unscheduled_symmetric_sum_clamp)
        else {
            return;
        };
        if self
            .output
            .relocations
            .iter()
            .any(|relocation| (start..start + 19).contains(&relocation.instruction_index))
        {
            return;
        }

        let (first, sum, bound) = match (
            &self.output.instructions[start],
            &self.output.instructions[start + 2],
            &self.output.instructions[start + 3],
        ) {
            (
                Instruction::LoadFloatSingle { d: first, .. },
                Instruction::FloatAddSingle { d: sum, .. },
                Instruction::FloatCompareOrdered { b: bound, .. },
            ) => (*first, *sum, *bound),
            _ => unreachable!("the complete clamp stream was recognized"),
        };
        let negative_bound = 2;
        match &mut self.output.instructions[start + 6] {
            Instruction::FloatSubtractSingle { b, .. } => *b = first,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 12] {
            Instruction::FloatNegate { d, b } => {
                *d = negative_bound;
                *b = bound;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 13] {
            Instruction::FloatCompareOrdered { a, b } => {
                *a = sum;
                *b = negative_bound;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 17] {
            Instruction::FloatSubtractSingle { a, b, .. } => {
                *a = negative_bound;
                *b = first;
            }
            _ => unreachable!(),
        }

        for index in [16, 15, 11, 10, 9, 5] {
            self.remove_structured_condition_instruction(start + index);
        }
    }
}

fn is_unscheduled_symmetric_sum_clamp(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: first, a: first_base, offset: first_offset },
        Instruction::LoadFloatSingle { d: second, a: second_base, offset: second_offset },
        Instruction::FloatAddSingle { d: sum, a: sum_first, b: sum_second },
        Instruction::FloatCompareOrdered { a: compared_sum, b: bound },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadFloatSingle { d: then_first, a: then_first_base, offset: then_first_offset },
        Instruction::FloatSubtractSingle { d: then_result, a: then_bound, b: then_subtrahend },
        Instruction::StoreFloatSingle { s: then_stored, a: then_store_base, offset: then_store_offset },
        Instruction::Branch { .. },
        Instruction::LoadFloatSingle { d: lower_first, a: lower_first_base, offset: lower_first_offset },
        Instruction::LoadFloatSingle { d: lower_second, a: lower_second_base, offset: lower_second_offset },
        Instruction::FloatAddSingle { d: lower_sum, a: lower_sum_first, b: lower_sum_second },
        Instruction::FloatNegate { d: negative, b: negative_bound },
        Instruction::FloatCompareOrdered { a: lower_compared_sum, b: lower_compared_bound },
        Instruction::BranchConditionalForward { .. },
        Instruction::FloatNegate { d: assignment_negative, b: assignment_bound },
        Instruction::LoadFloatSingle { d: assignment_first, a: assignment_first_base, offset: assignment_first_offset },
        Instruction::FloatSubtractSingle { d: assignment_result, a: assignment_left, b: assignment_right },
        Instruction::StoreFloatSingle { s: assignment_stored, a: assignment_store_base, offset: assignment_store_offset },
    ] if first == sum_first
        && second == sum_second
        && sum == compared_sum
        && first_base == then_first_base
        && first_base == lower_first_base
        && first_base == assignment_first_base
        && first_offset == then_first_offset
        && first_offset == lower_first_offset
        && first_offset == assignment_first_offset
        && second_base == then_store_base
        && second_base == lower_second_base
        && second_base == assignment_store_base
        && second_offset == then_store_offset
        && second_offset == lower_second_offset
        && second_offset == assignment_store_offset
        && then_first == then_subtrahend
        && bound == then_bound
        && then_result == then_stored
        && lower_first == lower_sum_first
        && lower_second == lower_sum_second
        && lower_sum == lower_compared_sum
        && negative == lower_compared_bound
        && bound == negative_bound
        && bound == assignment_bound
        && assignment_negative == assignment_left
        && assignment_first == assignment_right
        && assignment_result == assignment_stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_reloaded_symmetric_sum_clamp() {
        let instructions = [
            Instruction::LoadFloatSingle { d: 1, a: 1, offset: 24 },
            Instruction::LoadFloatSingle { d: 0, a: 31, offset: 252 },
            Instruction::FloatAddSingle { d: 0, a: 1, b: 0 },
            Instruction::FloatCompareOrdered { a: 0, b: 31 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 9 },
            Instruction::LoadFloatSingle { d: 0, a: 1, offset: 24 },
            Instruction::FloatSubtractSingle { d: 0, a: 31, b: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 252 },
            Instruction::Branch { target: 19 },
            Instruction::LoadFloatSingle { d: 1, a: 1, offset: 24 },
            Instruction::LoadFloatSingle { d: 0, a: 31, offset: 252 },
            Instruction::FloatAddSingle { d: 1, a: 1, b: 0 },
            Instruction::FloatNegate { d: 0, b: 31 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 19 },
            Instruction::FloatNegate { d: 1, b: 31 },
            Instruction::LoadFloatSingle { d: 0, a: 1, offset: 24 },
            Instruction::FloatSubtractSingle { d: 0, a: 1, b: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 252 },
        ];
        assert!(is_unscheduled_symmetric_sum_clamp(&instructions));
    }

    #[test]
    fn rejects_a_partial_sum_clamp() {
        let instructions = vec![Instruction::BranchToLinkRegister; 19];
        assert!(!is_unscheduled_symmetric_sum_clamp(&instructions));
    }
}
