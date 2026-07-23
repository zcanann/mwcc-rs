//! Cross-term schedules for structured short-circuit conditions.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Collapse two nested nonnull checks of the same member address into the
    /// receiver-producing record add plus a plain second test that MWCC keeps
    /// for the inlined wrapper boundary. The final direct call then consumes
    /// r3 without rematerializing the address.
    pub(super) fn schedule_repeated_member_address_call_guards(&mut self) {
        let mut start = 0;
        while start + 7 <= self.output.instructions.len() {
            if !is_repeated_member_address_call(&self.output.instructions[start..start + 7]) {
                start += 1;
                continue;
            }
            let (base, immediate) = match self.output.instructions[start] {
                Instruction::AddImmediateCarryingRecord { a, immediate, .. } => (a, immediate),
                _ => unreachable!(),
            };
            self.output.instructions[start] = Instruction::AddImmediateCarryingRecord {
                d: 3,
                a: base,
                immediate,
            };
            self.output.instructions[start + 2] =
                Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 };
            self.remove_structured_condition_instruction(start + 4);
            start += 6;
        }
    }

    fn remove_structured_condition_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed(at, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > at =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }

    /// Reuse a nested member base loaded by the preceding `&&` term. The first
    /// false-edge branch does not clobber the loaded pointer on fallthrough, so
    /// a byte/word member test followed by another member test can share it.
    pub(super) fn reuse_short_circuit_member_base(
        &mut self,
        term_index: usize,
        term_start: usize,
    ) {
        if term_index == 0
            || !reuses_preceding_member_load(&self.output.instructions, term_start)
            || self
                .output
                .relocations
                .iter()
                .any(|relocation| relocation.instruction_index == term_start)
        {
            return;
        }
        self.output.instructions.remove(term_start);
        self.labels.removed(term_start, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > term_start {
                relocation.instruction_index -= 1;
            }
        }
    }
}

fn is_repeated_member_address_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::AddImmediateCarryingRecord { d: 0, a: first_base, immediate: first_offset },
        Instruction::BranchConditionalForward { .. },
        Instruction::AddImmediateCarryingRecord { d: 0, a: second_base, immediate: second_offset },
        Instruction::BranchConditionalForward { .. },
        Instruction::AddImmediate { d: 3, a: call_base, immediate: call_offset },
        Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
        Instruction::BranchAndLink { .. },
    ] if first_base == second_base
        && first_base == call_base
        && first_offset == second_offset
        && first_offset == call_offset)
}

fn reuses_preceding_member_load(instructions: &[Instruction], term_start: usize) -> bool {
    let Some(previous) = term_start.checked_sub(4) else {
        return false;
    };
    let Some([
        Instruction::LoadWord {
            d: previous_result,
            a: previous_base,
            offset: previous_offset,
        },
        Instruction::LoadByteZero { a: tested_base, .. },
        Instruction::CompareLogicalWordImmediate { .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord {
            d: current_result,
            a: current_base,
            offset: current_offset,
        },
        ..
    ]) = instructions.get(previous..)
    else {
        return false;
    };
    previous_result == current_result
        && previous_base == current_base
        && previous_offset == current_offset
        && tested_base == previous_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_member_base_live_across_the_first_false_edge() {
        let instructions = [
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 392,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 3,
                offset: 36,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 392,
            },
        ];
        assert!(reuses_preceding_member_load(&instructions, 4));
    }

    #[test]
    fn recognizes_nested_member_address_guards_feeding_a_call() {
        let instructions = [
            Instruction::AddImmediateCarryingRecord { d: 0, a: 31, immediate: 64 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 7 },
            Instruction::AddImmediateCarryingRecord { d: 0, a: 31, immediate: 64 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 7 },
            Instruction::AddImmediate { d: 3, a: 31, immediate: 64 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::BranchAndLink { target: "__dt__6CTokenFv".to_string() },
        ];
        assert!(is_repeated_member_address_call(&instructions));
    }
}
