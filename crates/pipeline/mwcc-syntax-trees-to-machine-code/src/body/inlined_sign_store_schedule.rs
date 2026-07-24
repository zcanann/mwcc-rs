//! Lifetime scheduling for an inlined sign-selected member store.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Keep a member value live across an outer absolute-value guard and an
    /// inlined sign select.  The generic stream reuses r3 for a shared global
    /// before reloading the member through r3; retaining the original value in
    /// f2 both removes that unsafe reload and reproduces MWCC's leaf schedule.
    pub(crate) fn schedule_inlined_sign_store(&mut self) {
        if let Some(start) = self
            .output
            .instructions
            .windows(13)
            .position(is_guarded_inlined_sign_store)
        {
            for index in [start + 9, start + 11] {
                match &mut self.output.instructions[index] {
                    Instruction::FloatMove { d, .. } | Instruction::FloatNegate { d, .. } => {
                        *d = 0;
                    }
                    _ => unreachable!(),
                }
            }
            match &mut self.output.instructions[start + 12] {
                Instruction::StoreFloatSingle { s, .. } => *s = 0,
                _ => unreachable!(),
            }
        }

        let Some(start) = self
            .output
            .instructions
            .windows(21)
            .position(is_inlined_sign_store)
        else {
            return;
        };
        if !self
            .output
            .relocations
            .iter()
            .any(|relocation| relocation.instruction_index == start)
        {
            return;
        }

        match &mut self.output.instructions[start + 1] {
            Instruction::LoadFloatSingle { d, .. } => *d = 2,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 2] {
            Instruction::FloatCompareOrdered { a, .. } => *a = 2,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 4] {
            Instruction::FloatNegate { d, b } => {
                *d = 1;
                *b = 2;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 6] {
            Instruction::FloatMove { d, b } => {
                *d = 1;
                *b = 2;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 7] {
            Instruction::LoadWord { d, .. } => *d = 4,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 8] {
            Instruction::LoadFloatSingle { d, a, .. } => {
                *d = 0;
                *a = 4;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 9] {
            Instruction::FloatCompareOrdered { a, b } => {
                *a = 1;
                *b = 0;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 13] {
            Instruction::FloatCompareOrdered { a, b } => {
                *a = 2;
                *b = 0;
            }
            _ => unreachable!(),
        }
        for index in [start + 16, start + 18] {
            match &mut self.output.instructions[index] {
                Instruction::LoadFloatSingle { d, .. } => *d = 0,
                _ => unreachable!(),
            }
        }
        match &mut self.output.instructions[start + 19] {
            Instruction::StoreFloatSingle { s, .. } => *s = 0,
            _ => unreachable!(),
        }

        self.remove_structured_condition_instruction(start + 11);
        self.output.instructions.swap(start, start + 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index == start {
                relocation.instruction_index = start + 1;
            }
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn is_guarded_inlined_sign_store(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: 1, a: member_base, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::FloatCompareUnordered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadFloatSingle { d: 1, a: stack_base, .. },
        Instruction::FloatCompareUnordered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: exit, .. },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: negative, .. },
        Instruction::FloatMove { d: 1, b: positive_source },
        Instruction::Branch { target: store },
        Instruction::FloatNegate { d: 1, b: negative_source },
        Instruction::StoreFloatSingle { s: 1, a: store_base, .. },
    ] if *stack_base == 1
        && positive_source == negative_source
        && member_base == store_base
        && negative < store
        && store < exit)
}

fn is_inlined_sign_store(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::LoadFloatSingle { d: member, a: receiver, offset: member_offset },
        Instruction::FloatCompareOrdered { a: compare_member, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::FloatNegate { d: negative, b: negative_source },
        Instruction::Branch { .. },
        Instruction::FloatMove { d: positive, b: positive_source },
        Instruction::LoadWord { d: global, a: 0, .. },
        Instruction::LoadFloatSingle { d: threshold, a: global_base, .. },
        Instruction::FloatCompareOrdered { a: absolute, b: threshold_compare },
        Instruction::BranchConditionalToLinkRegister { .. },
        Instruction::LoadFloatSingle { d: reload, a: reload_base, offset: reload_offset },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::FloatCompareOrdered { a: sign_value, b: 0 },
        _,
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadFloatSingle { d: true_value, a: 0, .. },
        Instruction::Branch { .. },
        Instruction::LoadFloatSingle { d: false_value, a: 0, .. },
        Instruction::StoreFloatSingle { s: stored, a: store_base, .. },
        Instruction::BranchToLinkRegister,
    ] if member == compare_member
        && member == negative_source
        && member == positive_source
        && negative == positive
        && negative == absolute
        && global == global_base
        && threshold == threshold_compare
        && reload == sign_value
        && receiver == reload_base
        && receiver == store_base
        && member_offset == reload_offset
        && true_value == false_value
        && true_value == stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_outer_absolute_guard_with_an_inlined_sign_store() {
        let instructions = [
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 1568 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 6 },
            Instruction::FloatNegate { d: 0, b: 1 },
            Instruction::Branch { target: 7 },
            Instruction::FloatMove { d: 0, b: 1 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 1 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 1568 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 18 },
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::Branch { target: 19 },
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 1, a: 3, offset: 44 },
            Instruction::BranchToLinkRegister,
        ];

        assert!(is_inlined_sign_store(&instructions));
    }

    #[test]
    fn recognizes_a_guarded_nonleaf_sign_store() {
        let instructions = [
            Instruction::LoadFloatSingle { d: 1, a: 31, offset: 252 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareUnordered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 13 },
            Instruction::LoadFloatSingle { d: 1, a: 1, offset: 24 },
            Instruction::FloatCompareUnordered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 13 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 11 },
            Instruction::FloatMove { d: 1, b: 30 },
            Instruction::Branch { target: 12 },
            Instruction::FloatNegate { d: 1, b: 30 },
            Instruction::StoreFloatSingle { s: 1, a: 31, offset: 252 },
        ];
        assert!(is_guarded_inlined_sign_store(&instructions));
    }
}
