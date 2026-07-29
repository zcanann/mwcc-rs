//! Reuse a guarded pointer member load as the guarded call's first argument.
//!
//! Selection can allocate a null-test-only value to r0 and then reload the
//! identical member into r3 on the taken path.  Nothing can mutate the member
//! between those operations, so MWCC keeps the first load in r3 for both uses.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_guarded_call_pointer_loads(&mut self) {
        while let Some((load, reload)) =
            guarded_call_pointer_reload(&self.output.instructions)
        {
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[load] else {
                unreachable!("the guarded pointer load was matched")
            };
            *d = Eabi::general_result().number;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[load + 1]
            else {
                unreachable!("the guarded pointer comparison was matched")
            };
            *a = Eabi::general_result().number;
            crate::remove_instruction_retargeting_to_next(self, reload);
        }
    }
}

fn guarded_call_pointer_reload(instructions: &[Instruction]) -> Option<(usize, usize)> {
    instructions.windows(5).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::LoadWord {
                    d: tested,
                    a: tested_base,
                    offset: tested_offset,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: compared,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: argument_base,
                    offset: argument_offset,
                },
                Instruction::BranchAndLink { .. },
            ] if tested == compared
                && *tested != 3
                && tested_base == argument_base
                && tested_offset == argument_offset
                && *target == start + 5 =>
            {
                Some((start, start + 3))
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guarded_reload(target: usize) -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 0,
                a: 30,
                offset: 20,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            },
            Instruction::LoadWord {
                d: 3,
                a: 30,
                offset: 20,
            },
            Instruction::BranchAndLink {
                target: "release".into(),
            },
        ]
    }

    #[test]
    fn recognizes_a_reload_guarded_around_one_call() {
        assert_eq!(guarded_call_pointer_reload(&guarded_reload(5)), Some((0, 3)));
    }

    #[test]
    fn preserves_a_reload_when_the_branch_has_another_join() {
        assert_eq!(guarded_call_pointer_reload(&guarded_reload(6)), None);
    }
}
