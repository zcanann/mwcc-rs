//! Register and load scheduling for a guarded member clamp followed by a
//! two-component projection.
//!
//! The generic expression path evaluates each global bound independently. In
//! this complete region that both reloads the bound and lets the global pointer
//! overwrite the object receiver. MWCC keeps the receiver in r3, uses r4 for
//! each global access, and reuses the loaded bound in each conditional store.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_ground_knockback_projection(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(38)
            .position(is_unscheduled_projection)
        else {
            return;
        };

        let relocated = self
            .output
            .relocations
            .iter()
            .filter(|relocation| (start..start + 38).contains(&relocation.instruction_index))
            .map(|relocation| relocation.instruction_index - start)
            .collect::<Vec<_>>();
        if relocated != [3, 11, 15, 19, 24]
            || ![15, 19, 24].into_iter().all(|index| {
                schedule_relocations::same_relocated_value(
                    &self.output.relocations,
                    &self.output.constants,
                    start + 11,
                    start + index,
                )
            })
        {
            return;
        }

        let old = self.output.instructions[start..start + 38].to_vec();
        let mut replacement = Vec::with_capacity(33);
        replacement.extend_from_slice(&old[..7]);
        replacement.push(old[8].clone());
        replacement.push(old[7].clone());
        replacement.push(old[9].clone());

        let mut first_global = old[11].clone();
        let Instruction::LoadWord { d, .. } = &mut first_global else { unreachable!() };
        *d = 4;
        replacement.push(first_global);

        let mut first_member = old[10].clone();
        let Instruction::LoadFloatSingle { d, .. } = &mut first_member else { unreachable!() };
        *d = 0;
        replacement.push(first_member);

        let mut upper_bound = old[12].clone();
        let Instruction::LoadFloatSingle { d, a, .. } = &mut upper_bound else { unreachable!() };
        *d = 1;
        *a = 4;
        replacement.push(upper_bound);
        replacement.push(Instruction::FloatCompareOrdered { a: 0, b: 1 });
        replacement.push(Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: start + 16,
        });
        let mut upper_store = old[17].clone();
        let Instruction::StoreFloatSingle { s, .. } = &mut upper_store else { unreachable!() };
        *s = 1;
        replacement.push(upper_store);

        let mut second_global = old[19].clone();
        let Instruction::LoadWord { d, .. } = &mut second_global else { unreachable!() };
        *d = 4;
        replacement.push(second_global);
        replacement.push(old[18].clone());
        let mut lower_bound = old[20].clone();
        let Instruction::LoadFloatSingle { a, .. } = &mut lower_bound else { unreachable!() };
        *a = 4;
        replacement.push(lower_bound);
        replacement.extend_from_slice(&old[21..24]);
        let Instruction::BranchConditionalForward { target, .. } = &mut replacement[21] else {
            unreachable!()
        };
        *target = start + 23;
        replacement.push(old[27].clone());
        replacement.extend_from_slice(&old[28..]);
        debug_assert_eq!(replacement.len(), 33);

        self.output.instructions.splice(start..start + 38, replacement);
        self.output.relocations.retain(|relocation| {
            ![start + 15, start + 24].contains(&relocation.instruction_index)
        });
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 11 => start + 10,
                index if index == start + 19 => start + 16,
                index if index >= start + 38 => index - 5,
                index => index,
            };
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn is_unscheduled_projection(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: 0, .. },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalToLinkRegister { .. },
        Instruction::LoadFloatSingle { d: 1, a: 0, .. },
        Instruction::LoadFloatSingle { d: 0, a: receiver, offset: member_offset },
        Instruction::FloatCompareUnordered { a: 1, b: 0 },
        Instruction::BranchConditionalToLinkRegister { .. },
        Instruction::AddImmediate { d: normal, a: add_base, .. },
        Instruction::LoadFloatSingle { d: 0, a: source_base, .. },
        Instruction::StoreFloatSingle { s: 0, a: store_base, offset: store_offset },
        Instruction::LoadFloatSingle { d: 1, a: reload_base, offset: reload_offset },
        Instruction::LoadWord { d: 3, a: 0, .. },
        Instruction::LoadFloatSingle { d: 0, a: 3, offset: upper_offset },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 4, a: 0, .. },
        Instruction::LoadFloatSingle { d: 0, a: 4, offset: duplicate_upper },
        Instruction::StoreFloatSingle { s: 0, a: first_store_base, offset: first_store_offset },
        Instruction::LoadFloatSingle { d: 1, a: second_reload_base, offset: second_reload_offset },
        Instruction::LoadWord { d: 3, a: 0, .. },
        Instruction::LoadFloatSingle { d: 0, a: 3, offset: lower_offset },
        Instruction::FloatNegate { d: 0, b: 0 },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 4, a: 0, .. },
        Instruction::LoadFloatSingle { d: 0, a: 4, offset: duplicate_lower },
        Instruction::FloatNegate { d: 0, b: 0 },
        Instruction::StoreFloatSingle { s: 0, a: second_store_base, offset: second_store_offset },
        Instruction::LoadFloatSingle { d: 1, a: product_base_1, .. },
        Instruction::LoadFloatSingle { d: 0, a: product_base_2, offset: product_offset_1 },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: product_store_1, .. },
        Instruction::LoadFloatSingle { d: 1, a: product_base_3, .. },
        Instruction::LoadFloatSingle { d: 0, a: product_base_4, offset: product_offset_2 },
        Instruction::FloatNegate { d: 1, b: 1 },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: product_store_2, .. },
        Instruction::BranchToLinkRegister,
    ] if receiver == add_base
        && add_base == source_base
        && source_base == store_base
        && store_base == reload_base
        && reload_base == first_store_base
        && first_store_base == second_reload_base
        && second_reload_base == second_store_base
        && second_store_base == product_base_2
        && product_base_2 == product_store_1
        && product_store_1 == product_base_4
        && product_base_4 == product_store_2
        && normal == product_base_1
        && product_base_1 == product_base_3
        && member_offset == store_offset
        && store_offset == reload_offset
        && reload_offset == first_store_offset
        && first_store_offset == second_reload_offset
        && second_reload_offset == second_store_offset
        && upper_offset == duplicate_upper
        && upper_offset == lower_offset
        && lower_offset == duplicate_lower
        && product_offset_1 == member_offset
        && product_offset_2 == member_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_distinct_receiver_and_ground_normal_bases() {
        let mut instructions = vec![
            Instruction::LoadWord { d: 0, a: 3, offset: 224 },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 2 },
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 240 },
            Instruction::FloatCompareUnordered { a: 1, b: 0 },
            Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 2 },
            Instruction::AddImmediate { d: 5, a: 3, immediate: 2116 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 140 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 240 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 240 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 356 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 1, target: 18 },
            Instruction::LoadWord { d: 4, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 356 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 240 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 240 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 356 },
            Instruction::FloatNegate { d: 0, b: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 28 },
            Instruction::LoadWord { d: 4, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 356 },
            Instruction::FloatNegate { d: 0, b: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 240 },
            Instruction::LoadFloatSingle { d: 1, a: 5, offset: 4 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 240 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 140 },
            Instruction::LoadFloatSingle { d: 1, a: 5, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 240 },
            Instruction::FloatNegate { d: 1, b: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 144 },
            Instruction::BranchToLinkRegister,
        ];

        assert!(is_unscheduled_projection(&instructions));
        let Instruction::LoadFloatSingle { a, .. } = &mut instructions[28] else {
            unreachable!();
        };
        *a = 6;
        assert!(!is_unscheduled_projection(&instructions));
    }
}
