//! Scheduling for adjacent float-member products with one shared factor.
//!
//! For `out.x = a * scale; out.y = -b * scale`, mwcc overlaps the first
//! independent loads by fetching the shared factor before `a`. Generic
//! expression lowering follows source operand order, so this small scheduler
//! owns the complete two-store window and changes only that independent pair.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_shared_right_float_product_pair(&mut self) {
        let Some(start) = self.output.instructions
            .windows(9)
            .position(is_unscheduled_product_pair)
        else {
            return;
        };

        // Both loads are independent and neither carries a relocation. Their
        // destination registers stay attached to the values, so the multiply
        // needs no rewrite after exchanging them.
        self.output.instructions.swap(start, start + 1);
    }
}

fn is_unscheduled_product_pair(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: first_unique, a: first_base, offset: first_offset },
        Instruction::LoadFloatSingle { d: shared, a: shared_base, offset: shared_offset },
        Instruction::FloatMultiplySingle { d: first_product, a: first_factor, c: first_scale },
        Instruction::StoreFloatSingle { s: first_stored, a: first_target, offset: first_target_offset },
        Instruction::LoadFloatSingle { d: second_unique, a: second_base, offset: second_offset },
        Instruction::LoadFloatSingle { d: shared_again, a: shared_base_again, offset: shared_offset_again },
        Instruction::FloatNegate { d: negated, b: negated_source },
        Instruction::FloatMultiplySingle { d: second_product, a: second_factor, c: second_scale },
        Instruction::StoreFloatSingle { s: second_stored, a: second_target, offset: second_target_offset },
    ] if first_unique != shared
        && first_base == shared_base
        && first_offset != shared_offset
        && first_product == shared
        && first_factor == first_unique
        && first_scale == shared
        && first_stored == shared
        && second_unique != shared
        && second_base == first_base
        && second_offset != first_offset
        && shared_again == shared
        && shared_base_again == shared_base
        && shared_offset_again == shared_offset
        && negated == second_unique
        && negated_source == second_unique
        && second_product == shared
        && second_factor == second_unique
        && second_scale == shared
        && second_stored == shared
        && second_target == first_target
        && first_target_offset.checked_add(4) == Some(*second_target_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_complete_unscheduled_pair() {
        let instructions = vec![
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 2120 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 244 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 152 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 2116 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 244 },
            Instruction::FloatNegate { d: 1, b: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 156 },
        ];
        assert!(is_unscheduled_product_pair(&instructions));
    }
}
