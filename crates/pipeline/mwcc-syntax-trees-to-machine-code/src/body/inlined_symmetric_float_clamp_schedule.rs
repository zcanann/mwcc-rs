//! Final scheduling for symmetric member clamps introduced by inline expansion.
//!
//! Whole-function clamp lowering owns the same lifetime directly. An expanded
//! helper reaches the physical stream through the ordinary statement walker,
//! which reloads the member and recomputes its negated bound in each arm.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_inlined_symmetric_float_clamp(&mut self) {
        let mut start = 0;
        while start + 12 <= self.output.instructions.len() {
            if !is_unscheduled_clamp(&self.output.instructions[start..start + 12]) {
                start += 1;
                continue;
            }
            if self
                .output
                .relocations
                .iter()
                .any(|relocation| (start..start + 12).contains(&relocation.instruction_index))
            {
                start += 1;
                continue;
            }

            let retained_member = match self.output.instructions[start + 2] {
                Instruction::LoadFloatSingle { d, .. } => d,
                _ => unreachable!(),
            };
            self.output.instructions.swap(start + 1, start + 2);
            match &mut self.output.instructions[start + 9] {
                Instruction::FloatCompareOrdered { a, .. } => *a = retained_member,
                _ => unreachable!(),
            }

            // Remove from the end so the second index remains stable. The
            // shared helper updates every later branch and relocation index.
            self.remove_structured_condition_instruction(start + 8);
            self.remove_structured_condition_instruction(start + 5);
            start += 10;
        }
    }
}

fn is_unscheduled_clamp(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: bound, a: bound_base, .. },
        Instruction::FloatNegate { d: negative, b: negative_bound },
        Instruction::LoadFloatSingle { d: member, a: member_base, offset: member_offset },
        Instruction::FloatCompareOrdered { a: compared_member, b: compared_negative },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 0, .. },
        Instruction::FloatNegate { d: duplicate_negative, b: duplicate_bound },
        Instruction::StoreFloatSingle { s: stored_negative, a: lower_base, offset: lower_offset },
        Instruction::Branch { .. },
        Instruction::LoadFloatSingle { d: reloaded, a: reload_base, offset: reload_offset },
        Instruction::FloatCompareOrdered { a: compared_reload, b: compared_bound },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 1, .. },
        Instruction::StoreFloatSingle { s: stored_bound, a: upper_base, offset: upper_offset },
    ] if bound == negative_bound
        && bound == duplicate_bound
        && bound == compared_bound
        && bound == stored_bound
        && negative == compared_negative
        && negative == duplicate_negative
        && negative == stored_negative
        && member == compared_member
        && reloaded == compared_reload
        && bound_base == member_base
        && member_base == lower_base
        && lower_base == reload_base
        && reload_base == upper_base
        && member_offset == lower_offset
        && lower_offset == reload_offset
        && reload_offset == upper_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_recomputed_symmetric_member_clamp() {
        let instructions = [
            Instruction::LoadFloatSingle { d: 1, a: 31, offset: 324 },
            Instruction::FloatNegate { d: 0, b: 1 },
            Instruction::LoadFloatSingle { d: 2, a: 31, offset: 236 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 8 },
            Instruction::FloatNegate { d: 0, b: 1 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 236 },
            Instruction::Branch { target: 12 },
            Instruction::LoadFloatSingle { d: 0, a: 31, offset: 236 },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 1, target: 12 },
            Instruction::StoreFloatSingle { s: 1, a: 31, offset: 236 },
        ];
        assert!(is_unscheduled_clamp(&instructions));
    }
}
