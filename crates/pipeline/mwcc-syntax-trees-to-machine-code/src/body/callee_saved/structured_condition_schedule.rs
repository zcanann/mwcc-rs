//! Cross-term schedules for structured short-circuit conditions.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
}
