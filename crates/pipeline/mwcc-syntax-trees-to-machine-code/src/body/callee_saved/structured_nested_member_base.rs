//! Retain a nested member base across a structured link-insertion diamond.
//!
//! A pointer loaded for `previous = owner->queue->tail` remains live through
//! the following null check so the then arm can write `owner->queue->head`
//! without reloading `owner->queue`. The join releases it; a later queue access
//! reloads normally. This pass operates before register allocation so the
//! retained base and `previous` have distinct live ranges and color like MWCC.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn retain_guarded_nested_member_base(&mut self) {
        while let Some((start, retained, value)) =
            guarded_nested_member_reload(&self.output.instructions)
        {
            let (value_preference, retained_preference) =
                if self.behavior.legacy_guarded_nested_member_base_order {
                    (
                        Eabi::FIRST_GENERAL_ARGUMENT + 1,
                        Eabi::FIRST_GENERAL_ARGUMENT + 2,
                    )
                } else {
                    (
                        Eabi::FIRST_GENERAL_ARGUMENT + 2,
                        Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    )
                };
            self.prefer_virtual_general(value, value_preference);
            self.prefer_virtual_general(retained, retained_preference);
            match &mut self.output.instructions[start] {
                Instruction::LoadWord { d, .. } => *d = retained,
                _ => unreachable!("shape checked"),
            }
            match &mut self.output.instructions[start + 1] {
                Instruction::LoadWord { a, .. } => *a = retained,
                _ => unreachable!("shape checked"),
            }
            self.remove_structured_condition_instruction(start + 4);
        }
    }
}

fn guarded_nested_member_reload(instructions: &[Instruction]) -> Option<(usize, u8, u8)> {
    instructions.windows(8).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::LoadWord {
                    d: chased,
                    a: root,
                    offset: base_offset,
                },
                Instruction::LoadWord {
                    d: value,
                    a: loaded_base,
                    ..
                },
                Instruction::CompareLogicalWordImmediate {
                    a: compared,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    target: else_target,
                    ..
                },
                Instruction::LoadWord {
                    d: retained,
                    a: reloaded_root,
                    offset: reload_offset,
                },
                Instruction::StoreWord {
                    s: then_value,
                    a: then_base,
                    ..
                },
                Instruction::Branch { target: join },
                Instruction::StoreWord {
                    s: else_value,
                    a: else_base,
                    ..
                },
            ] if chased == value
                && chased == loaded_base
                && value == compared
                && root == reloaded_root
                && base_offset == reload_offset
                && root == then_value
                && retained == then_base
                && root == else_value
                && value == else_base
                && *else_target == start + 7
                && *join >= start + 8 =>
            {
                Some((start, *retained, *value))
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_guarded_nested_member_reload() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 4,
            },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 3,
                a: 5,
                offset: 0,
            },
            Instruction::Branch { target: 8 },
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: 4,
            },
        ];

        assert_eq!(guarded_nested_member_reload(&instructions), Some((0, 5, 4)));
    }
}
