//! Entry-copy record forms whose CR0 value survives until a later null guard.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fold `mr saved,incoming; ...; cmplwi saved,0` into `mr.` when the
    /// intervening straight-line work cannot touch CR0 or redefine the saved
    /// pointer. MWCC keeps this entry result live across independent loads,
    /// floating arithmetic, and stores before an inlined null assertion.
    pub(crate) fn schedule_entry_saved_zero_test(&mut self) {
        let Some((copy, compare)) = find_entry_saved_zero_test(&self.output.instructions) else {
            return;
        };
        let (saved, incoming) = match self.output.instructions[copy] {
            Instruction::Or { a, s, b } if s == b => (a, s),
            _ => unreachable!(),
        };
        self.output.instructions[copy] = Instruction::OrRecord {
            a: saved,
            s: incoming,
            b: incoming,
        };
        self.remove_structured_condition_instruction(compare);
    }
}

fn find_entry_saved_zero_test(instructions: &[Instruction]) -> Option<(usize, usize)> {
    for (copy, instruction) in instructions.iter().enumerate() {
        let Instruction::Or {
            a: saved,
            s: incoming,
            b,
        } = *instruction
        else {
            continue;
        };
        if incoming != b || !(14..=31).contains(&saved) || !(3..=10).contains(&incoming) {
            continue;
        }

        for compare in copy + 1..instructions.len().saturating_sub(1) {
            if matches!(
                instructions[compare],
                Instruction::CompareLogicalWordImmediate { a, immediate: 0 } if a == saved
            ) && matches!(
                instructions[compare + 1],
                Instruction::BranchConditionalForward { condition_bit: 2, .. }
            ) {
                return Some((copy, compare));
            }
            if !preserves_entry_zero_record(&instructions[compare], saved) {
                break;
            }
        }
    }
    None
}

fn preserves_entry_zero_record(instruction: &Instruction, saved: u8) -> bool {
    let redefines_saved = mwcc_vreg::register_operands(instruction)
        .into_iter()
        .any(|operand| {
            operand.class == mwcc_vreg::Class::General
                && operand.role == mwcc_vreg::RegisterRole::Define
                && operand.register == saved
        });
    if redefines_saved {
        return false;
    }
    matches!(
        instruction,
        Instruction::LoadWord { .. }
            | Instruction::LoadByteZero { .. }
            | Instruction::LoadHalfwordZero { .. }
            | Instruction::LoadHalfwordAlgebraic { .. }
            | Instruction::LoadFloatSingle { .. }
            | Instruction::LoadFloatDouble { .. }
            | Instruction::LoadWordIndexed { .. }
            | Instruction::LoadByteZeroIndexed { .. }
            | Instruction::LoadHalfwordZeroIndexed { .. }
            | Instruction::LoadHalfwordAlgebraicIndexed { .. }
            | Instruction::LoadFloatSingleIndexed { .. }
            | Instruction::StoreWord { .. }
            | Instruction::StoreByte { .. }
            | Instruction::StoreHalfword { .. }
            | Instruction::StoreFloatSingle { .. }
            | Instruction::StoreFloatDouble { .. }
            | Instruction::StoreWordIndexed { .. }
            | Instruction::StoreByteIndexed { .. }
            | Instruction::StoreHalfwordIndexed { .. }
            | Instruction::StoreFloatSingleIndexed { .. }
            | Instruction::FloatAddSingle { .. }
            | Instruction::FloatSubtractSingle { .. }
            | Instruction::FloatMultiplySingle { .. }
            | Instruction::FloatDivideSingle { .. }
            | Instruction::FloatMultiplyAddSingle { .. }
            | Instruction::FloatMultiplySubtractSingle { .. }
            | Instruction::FloatNegativeMultiplyAddSingle { .. }
            | Instruction::FloatNegativeMultiplySubtractSingle { .. }
            | Instruction::FloatMove { .. }
            | Instruction::FloatNegate { .. }
            | Instruction::FloatAbsolute { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_an_entry_copy_across_cr_neutral_float_work() {
        let instructions = vec![
            Instruction::move_register(31, 4),
            Instruction::LoadWord { d: 3, a: 3, offset: 268 },
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::FloatDivideSingle { d: 0, a: 1, b: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 16 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 8,
            },
        ];

        assert_eq!(find_entry_saved_zero_test(&instructions), Some((0, 5)));
    }

    #[test]
    fn rejects_a_copy_when_intervening_work_changes_cr0() {
        let instructions = vec![
            Instruction::move_register(31, 4),
            Instruction::OrRecord { a: 0, s: 3, b: 3 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 5,
            },
        ];

        assert_eq!(find_entry_saved_zero_test(&instructions), None);
    }
}
