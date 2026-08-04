//! Schedule a loop body's narrow discriminator before its step-value prefetch.
//!
//! A pointer loop may load the next link before switching on the current
//! object's narrow state. MWCC issues the independent state load first, while
//! still retaining the next link across a call in the selected arm. This pass
//! runs after allocation, when that loop-carried role is explicit.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_loop_step_prefetch(&mut self) {
        let mut start = 0;
        while start + 1 < self.output.instructions.len() {
            if loop_step_prefetch_at(&self.output.instructions, start) {
                // Branches already target the numeric loop-body entry. Keeping
                // their indices unchanged makes the discriminator the new
                // entry instruction, matching MWCC's scheduled loop.
                self.output.instructions.swap(start, start + 1);
                start += 2;
            } else {
                start += 1;
            }
        }
    }
}

fn loop_step_prefetch_at(instructions: &[Instruction], start: usize) -> bool {
    let Some([
        Instruction::LoadWord {
            d: next,
            a: cursor,
            ..
        },
        Instruction::LoadHalfwordZero {
            d: discriminator,
            a: discriminator_base,
            ..
        },
    ]) = instructions.get(start..start + 2)
    else {
        return false;
    };
    if next == cursor || cursor != discriminator_base || *discriminator != 0 {
        return false;
    }

    let end = instructions.len().min(start + 18);
    let tail = &instructions[start + 2..end];
    let Some(copy) = tail.iter().position(|instruction| {
        matches!(instruction,
            Instruction::Or { a, s, b }
                if a == cursor && s == next && b == next)
            || matches!(instruction,
                Instruction::AddImmediate { d, a, immediate: 0 }
                    if d == cursor && a == next)
    }) else {
        return false;
    };
    tail[..copy]
        .iter()
        .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        && instructions[start + 2 + copy..]
            .iter()
            .any(|instruction| {
                matches!(instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        if *target == start)
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_called_switch_loop_with_a_prefetched_next_link() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 764,
            },
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 712,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 4 },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
            Instruction::move_register(3, 31),
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            },
        ];

        assert!(loop_step_prefetch_at(&instructions, 0));
    }

    #[test]
    fn rejects_a_prefetch_without_a_loop_backedge() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 8,
            },
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 4,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
            Instruction::move_register(3, 31),
        ];

        assert!(!loop_step_prefetch_at(&instructions, 0));
    }
}
