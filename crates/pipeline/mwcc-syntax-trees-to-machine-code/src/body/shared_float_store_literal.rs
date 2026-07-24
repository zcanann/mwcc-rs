//! Reuse of a float literal across adjacent member-product groups.
//!
//! A pair of vector Z stores separated by two X/Y product stores keeps the
//! common literal in the first free FPR. Per-statement lowering reloads it into
//! the arithmetic scratch register; this final physical-stream owner proves
//! the complete straight-line region before extending the literal lifetime.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_shared_float_store_literal(&mut self, function: &Function) {
        if !is_repeated_zero_store_body(function) {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(13)
            .position(is_reloaded_float_store_literal)
        else {
            return;
        };
        let reload = start + 11;
        if !schedule_relocations::same_relocated_value(
            &self.output.relocations,
            &self.output.constants,
            start,
            reload,
        ) || self.output.instructions.iter().any(|instruction| matches!(instruction,
            Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target }
                if (start..start + 13).contains(target)
        )) {
            return;
        }

        match &mut self.output.instructions[start] {
            Instruction::LoadFloatSingle { d, .. } => *d = 2,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 1] {
            Instruction::StoreFloatSingle { s, .. } => *s = 2,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 12] {
            Instruction::StoreFloatSingle { s, .. } => *s = 2,
            _ => unreachable!(),
        }
        self.output.instructions.remove(reload);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != reload);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > reload {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > reload =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }
}

fn is_repeated_zero_store_body(function: &Function) -> bool {
    if function_makes_call(function)
        || function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double))
        || function
            .locals
            .iter()
            .any(|local| matches!(local.declared_type, Type::Float | Type::Double))
    {
        return false;
    }
    let [_, _, first_zero, _, _, second_zero] = function.statements.as_slice() else {
        return false;
    };
    let (
        Statement::Store {
            target:
                Expression::Member {
                    base: first_base,
                    offset: first_offset,
                    member_type: Type::Float,
                    index_stride: None,
                },
            value: first_value,
        },
        Statement::Store {
            target:
                Expression::Member {
                    base: second_base,
                    offset: second_offset,
                    member_type: Type::Float,
                    index_stride: None,
                },
            value: second_value,
        },
    ) = (first_zero, second_zero)
    else {
        return false;
    };
    is_zero(first_value)
        && is_zero(second_value)
        && first_offset.checked_add(12) == Some(*second_offset)
        && structurally_equal(first_base, second_base)
}

fn is_zero(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
        || matches!(expression, Expression::FloatLiteral(value) if *value == 0.0)
}

fn is_reloaded_float_store_literal(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: first_base, offset: first_zero_offset },
        Instruction::LoadFloatSingle { d: 1, a: product_base_1, .. },
        Instruction::LoadFloatSingle { d: 0, a: product_base_2, .. },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: product_store_1, .. },
        Instruction::LoadFloatSingle { d: 1, a: product_base_3, .. },
        Instruction::LoadFloatSingle { d: 0, a: product_base_4, .. },
        Instruction::FloatNegate { d: 1, b: 1 },
        Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
        Instruction::StoreFloatSingle { s: 0, a: product_store_2, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: second_base, offset: second_zero_offset },
    ] if first_base == product_base_1
        && product_base_1 == product_base_2
        && product_base_2 == product_store_1
        && product_store_1 == product_base_3
        && product_base_3 == product_base_4
        && product_base_4 == product_store_2
        && product_store_2 == second_base
        && first_zero_offset.checked_add(12) == Some(*second_zero_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_literal_reloaded_across_an_xy_product_pair() {
        let instructions = [
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 124 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 2120 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 236 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 128 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 2116 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 236 },
            Instruction::FloatNegate { d: 1, b: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 132 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 4, offset: 136 },
        ];
        assert!(is_reloaded_float_store_literal(&instructions));
    }
}
