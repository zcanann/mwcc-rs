//! Delay an entry-member saved-home promotion through its first guard load.
//!
//! The selector may load an entry member directly into its call-live saved
//! home, then restore the ABI entry register for a dependent guard load. Build
//! 163 keeps the member in that volatile register through the guard load and
//! promotes it only after the load has issued.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_entry_member_guard_home(&mut self) -> bool {
        let Some((start, saved, entry)) = entry_member_guard_home(&self.output.instructions) else {
            return false;
        };

        self.move_instruction_before(start + 2, start + 1);
        match &mut self.output.instructions[start] {
            Instruction::LoadWord { d, .. } => *d = entry,
            _ => unreachable!("entry member load changed after recognition"),
        }
        self.output.instructions[start + 2] = Instruction::AddImmediate {
            d: saved,
            a: entry,
            immediate: 0,
        };
        true
    }
}

fn entry_member_guard_home(instructions: &[Instruction]) -> Option<(usize, u8, u8)> {
    instructions
        .windows(5)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadWord {
                d: saved, a: entry, ..
            }, Instruction::Or {
                a: restored_entry,
                s: copied_saved,
                b: copied_saved_again,
            }, Instruction::LoadWord {
                d: tested,
                a: guard_base,
                ..
            }, Instruction::CompareLogicalWordImmediate {
                a: compared,
                immediate: 0,
            }, Instruction::BranchConditionalForward { .. }] = window
            else {
                return None;
            };
            if !(14..=31).contains(saved)
                || !(3..=10).contains(entry)
                || restored_entry != entry
                || copied_saved != saved
                || copied_saved_again != saved
                || guard_base != entry
                || tested != compared
                || instructions[..start]
                    .iter()
                    .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
                || !saved_home_is_live_past_a_call(instructions, start + 5, *saved)
            {
                return None;
            }
            Some((start, *saved, *entry))
        })
}

fn saved_home_is_live_past_a_call(
    instructions: &[Instruction],
    tail_start: usize,
    saved: u8,
) -> bool {
    let Some(first_call) = instructions[tail_start..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .map(|offset| tail_start + offset)
    else {
        return false;
    };
    instructions[first_call + 1..].iter().any(|instruction| {
        mwcc_vreg::register_operands(instruction)
            .into_iter()
            .any(|operand| {
                operand.class == mwcc_vreg::Class::General
                    && operand.role == mwcc_vreg::RegisterRole::Use
                    && operand.register == saved
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_guarded_entry_member_live_across_a_call() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 44,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 6524,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 7,
            },
            Instruction::BranchAndLink {
                target: "mutate".into(),
            },
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 12,
            },
        ];

        assert_eq!(entry_member_guard_home(&instructions), Some((0, 31, 3)));
    }

    #[test]
    fn rejects_a_saved_home_dead_after_the_first_call() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 44,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 6524,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 7,
            },
            Instruction::BranchAndLink {
                target: "mutate".into(),
            },
        ];
        instructions.push(Instruction::BranchToLinkRegister);

        assert_eq!(entry_member_guard_home(&instructions), None);
    }
}
