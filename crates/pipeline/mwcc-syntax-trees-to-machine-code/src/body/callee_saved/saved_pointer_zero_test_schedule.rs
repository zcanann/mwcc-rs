//! Hoist a saved-pointer null test into a preceding aggregate-load latency slot.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// An inlined aggregate getter can leave a second assertion on the source
    /// pointer after a long CR-neutral vector update. Build 163 starts that
    /// null test between the first two aggregate word loads and carries CR0
    /// across the intervening loads, stores, and floating arithmetic.
    pub(crate) fn schedule_saved_pointer_zero_test(&mut self) {
        let Some((from, to)) = find_saved_pointer_zero_test(&self.output.instructions) else {
            return;
        };
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == from => to,
                index if (to..from).contains(&index) => index + 1,
                index => index,
            };
        }
    }
}

fn find_saved_pointer_zero_test(instructions: &[Instruction]) -> Option<(usize, usize)> {
    for compare in 0..instructions.len().saturating_sub(1) {
        let Instruction::CompareLogicalWordImmediate {
            a: saved,
            immediate: 0,
        } = instructions[compare]
        else {
            continue;
        };
        if !(14..=31).contains(&saved)
            || !matches!(
                instructions[compare + 1],
                Instruction::BranchConditionalForward { condition_bit: 2, .. }
            )
        {
            continue;
        }
        let Some(assertion_call) = instructions[..compare]
            .iter()
            .rposition(|instruction| {
                matches!(instruction, Instruction::BranchAndLink { target } if target == "__assert")
            })
        else {
            continue;
        };
        let start = assertion_call + 1;
        let Some([
            Instruction::LoadWord { a: first_base, .. },
            Instruction::LoadWord { a: second_base, .. },
            ..,
        ]) = instructions.get(start..compare)
        else {
            continue;
        };
        if *first_base != saved
            || *second_base != saved
            || !instructions[start + 1..compare]
                .iter()
                .all(|instruction| preserves_saved_zero_record(instruction, saved))
        {
            continue;
        }
        return Some((compare, start + 1));
    }
    None
}

fn preserves_saved_zero_record(instruction: &Instruction, saved: u8) -> bool {
    let redefines_saved = mwcc_vreg::register_operands(instruction)
        .into_iter()
        .any(|operand| {
            operand.class == mwcc_vreg::Class::General
                && operand.role == mwcc_vreg::RegisterRole::Define
                && operand.register == saved
        });
    !redefines_saved
        && matches!(
            instruction,
            Instruction::LoadWord { .. }
                | Instruction::LoadFloatSingle { .. }
                | Instruction::LoadFloatDouble { .. }
                | Instruction::StoreWord { .. }
                | Instruction::StoreFloatSingle { .. }
                | Instruction::StoreFloatDouble { .. }
                | Instruction::FloatAddSingle { .. }
                | Instruction::FloatSubtractSingle { .. }
                | Instruction::FloatMultiplySingle { .. }
                | Instruction::FloatMultiplyAddSingle { .. }
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoists_a_later_null_test_between_adjacent_aggregate_loads() {
        let instructions = vec![
            Instruction::BranchAndLink { target: "__assert".into() },
            Instruction::LoadWord { d: 3, a: 31, offset: 56 },
            Instruction::LoadWord { d: 0, a: 31, offset: 60 },
            Instruction::StoreWord { s: 3, a: 1, offset: 16 },
            Instruction::LoadFloatSingle { d: 1, a: 1, offset: 16 },
            Instruction::FloatSubtractSingle { d: 0, a: 1, b: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 16 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 10,
            },
        ];

        assert_eq!(find_saved_pointer_zero_test(&instructions), Some((7, 2)));
    }

    #[test]
    fn stops_when_intervening_work_changes_cr0() {
        let mut instructions = vec![
            Instruction::BranchAndLink { target: "__assert".into() },
            Instruction::LoadWord { d: 3, a: 31, offset: 56 },
            Instruction::LoadWord { d: 0, a: 31, offset: 60 },
            Instruction::OrRecord { a: 0, s: 3, b: 3 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
        ];
        assert_eq!(find_saved_pointer_zero_test(&instructions), None);
        instructions[3] = Instruction::StoreWord { s: 3, a: 1, offset: 16 };
        assert_eq!(find_saved_pointer_zero_test(&instructions), Some((4, 2)));
    }
}
