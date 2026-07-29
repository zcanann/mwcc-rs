//! Register and load scheduling for a guarded member clamp followed by a
//! two-component projection.
//!
//! The generic true-edge cache now retains each global bound. This final
//! physical pass materializes the projection source address shared by its two
//! component loads. The older complete-region rewrite remains as a fallback
//! for input shapes produced before generic edge retention applies.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_ground_knockback_projection(&mut self) {
        if self.schedule_retained_bound_projection_address() {
            return;
        }
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

    /// Materialize a two-component source address once after generic guarded
    /// value retention has already eliminated the bound reloads.
    fn schedule_retained_bound_projection_address(&mut self) -> bool {
        let Some(start) = self
            .output
            .instructions
            .windows(32)
            .position(is_retained_bound_projection)
        else {
            return false;
        };
        if !schedule_relocations::same_relocated_value(
            &self.output.relocations,
            &self.output.constants,
            start + 9,
            start + 15,
        ) {
            return false;
        }
        let (receiver, normal_x_offset) = match (
            &self.output.instructions[start + 22],
            &self.output.instructions[start + 26],
        ) {
            (
                Instruction::LoadFloatSingle {
                    a: first_base,
                    offset: normal_y_offset,
                    ..
                },
                Instruction::LoadFloatSingle {
                    a: second_base,
                    offset: normal_x_offset,
                    ..
                },
            ) if first_base == second_base
                && normal_x_offset.checked_add(4) == Some(*normal_y_offset) =>
            {
                (*first_base, *normal_x_offset)
            }
            _ => return false,
        };

        crate::insert_instruction_retargeting(
            self,
            start + 8,
            Instruction::AddImmediate {
                d: 5,
                a: receiver,
                immediate: normal_x_offset as i16,
            },
        );
        for (index, offset) in [(start + 23, 4), (start + 27, 0)] {
            let Instruction::LoadFloatSingle {
                a,
                offset: load_offset,
                ..
            } = &mut self.output.instructions[index]
            else {
                unreachable!()
            };
            *a = 5;
            *load_offset = offset;
        }
        true
    }
}

fn is_retained_bound_projection(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: 0, .. },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalToLinkRegister { .. },
        Instruction::LoadFloatSingle { d: 1, .. },
        Instruction::LoadFloatSingle { d: 0, a: receiver, .. },
        Instruction::FloatCompareUnordered { a: 1, b: 0 },
        Instruction::BranchConditionalToLinkRegister { .. },
        Instruction::LoadFloatSingle { d: 0, a: source_base, offset: source_offset },
        Instruction::StoreFloatSingle { s: 0, a: first_store_base, offset: stored_offset },
        Instruction::LoadWord { d: 4, .. },
        Instruction::LoadFloatSingle { d: 0, a: first_reload_base, offset: first_reload_offset },
        Instruction::LoadFloatSingle { d: 1, a: 4, offset: upper_offset },
        Instruction::FloatCompareOrdered { a: 0, b: 1 },
        Instruction::BranchConditionalForward { .. },
        Instruction::StoreFloatSingle { s: 1, a: upper_store_base, offset: upper_store_offset },
        Instruction::LoadWord { d: 4, .. },
        Instruction::LoadFloatSingle { d: 1, a: second_reload_base, offset: second_reload_offset },
        Instruction::LoadFloatSingle { d: 0, a: 4, offset: lower_offset },
        Instruction::FloatNegate { d: 0, b: 0 },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::StoreFloatSingle { s: 0, a: lower_store_base, offset: lower_store_offset },
        Instruction::LoadFloatSingle { d: 1, a: normal_y_base, offset: normal_y_offset },
        Instruction::LoadFloatSingle { d: 0, a: first_product_base, offset: first_product_offset },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: first_result_base, offset: first_result_offset },
        Instruction::LoadFloatSingle { d: 1, a: normal_x_base, offset: normal_x_offset },
        Instruction::LoadFloatSingle { d: 0, a: second_product_base, offset: second_product_offset },
        Instruction::FloatNegate { d: 1, b: 1 },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: second_result_base, .. },
        Instruction::BranchToLinkRegister,
    ] if receiver == source_base
        && source_base == first_store_base
        && first_store_base == first_reload_base
        && first_reload_base == upper_store_base
        && upper_store_base == second_reload_base
        && second_reload_base == lower_store_base
        && lower_store_base == normal_y_base
        && normal_y_base == first_product_base
        && first_product_base == first_result_base
        && first_result_base == normal_x_base
        && normal_x_base == second_product_base
        && second_product_base == second_result_base
        && stored_offset == first_reload_offset
        && first_reload_offset == upper_store_offset
        && upper_store_offset == second_reload_offset
        && second_reload_offset == lower_store_offset
        && lower_store_offset == first_product_offset
        && first_product_offset == second_product_offset
        && source_offset == first_result_offset
        && upper_offset == lower_offset
        && normal_x_offset.checked_add(4) == Some(*normal_y_offset))
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
