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
            if self.preserve_guarded_named_local_values {
                preserve_guarded_call_pointer_value(
                    &mut self.output.instructions,
                    load,
                    reload,
                );
                continue;
            }
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
        if self.preserve_guarded_named_local_values {
            self.expand_guarded_named_local_calls();
        }
    }

    fn expand_guarded_named_local_calls(&mut self) {
        let mut start = 0;
        while start < self.output.instructions.len().saturating_sub(3) {
            if !is_direct_guarded_pointer_call(&self.output.instructions, start) {
                start += 1;
                continue;
            }
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start] else {
                unreachable!("the direct guarded pointer load was matched")
            };
            *d = GENERAL_SCRATCH;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[start + 1]
            else {
                unreachable!("the direct guarded pointer comparison was matched")
            };
            *a = GENERAL_SCRATCH;
            crate::insert_instruction_retargeting(
                self,
                start + 2,
                Instruction::move_register(Eabi::general_result().number, GENERAL_SCRATCH),
            );
            start += 5;
        }
    }
}

fn preserve_guarded_call_pointer_value(
    instructions: &mut [Instruction],
    load: usize,
    reload: usize,
) {
    let Instruction::LoadWord { d: source, .. } = instructions[load] else {
        unreachable!("the guarded pointer load was matched")
    };
    instructions[reload] = Instruction::move_register(Eabi::general_result().number, source);
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

fn is_direct_guarded_pointer_call(instructions: &[Instruction], start: usize) -> bool {
    matches!(
        instructions.get(start..start + 4),
        Some([
            Instruction::LoadWord { d: 3, .. },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            },
            Instruction::BranchAndLink { .. },
        ]) if *target >= start + 4
    )
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

    #[test]
    fn forwards_a_named_guard_value_instead_of_reloading_it() {
        let mut instructions = guarded_reload(5);
        preserve_guarded_call_pointer_value(&mut instructions, 0, 3);

        assert_eq!(instructions[3], Instruction::move_register(3, 0));
        assert_eq!(guarded_call_pointer_reload(&instructions), None);
    }

    #[test]
    fn recognizes_a_direct_guarded_pointer_call_for_named_local_expansion() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 28,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(is_direct_guarded_pointer_call(&instructions, 0));
    }
}
