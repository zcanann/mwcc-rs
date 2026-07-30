//! Guard-byte lifetime for pairwise object-collision loops.
//!
//! MWCC keeps the fighter-state byte live across the intervening ground and
//! victim-pointer guards. The retained byte removes a reload and lets the last
//! bit test branch directly to the loop continuation. This pass runs after
//! allocation because that lifetime also changes the floor-index load image.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_object_collision_loop_guard(&mut self) {
        if !self.structured_object_collision_loop_entry {
            return;
        }
        let Some(start) = allocated_object_collision_guard(&self.output.instructions) else {
            return;
        };

        if let Some(Instruction::Or { a: 3, s: 29, b: 29 }) =
            self.output.instructions.get(start.wrapping_sub(5))
        {
            self.output.instructions[start - 5] = Instruction::AddImmediate {
                d: 3,
                a: 29,
                immediate: 0,
            };
        }

        self.output.instructions[start] = Instruction::LoadWord {
            d: 4,
            a: 29,
            offset: 44,
        };
        self.output.instructions[start + 1] = Instruction::LoadByteZero {
            d: 3,
            a: 4,
            offset: 8735,
        };
        crate::insert_instruction_retargeting(
            self,
            start + 2,
            Instruction::AddImmediate {
                d: 24,
                a: 4,
                immediate: 0,
            },
        );
        let Instruction::RotateAndMaskRecord { s, .. } = &mut self.output.instructions[start + 3]
        else {
            unreachable!("the first bit test was matched")
        };
        *s = 3;

        // The insertion moves the redundant byte reload to +11. Removing it
        // leaves the second mask at +11 and its `beq; b exit` diamond at +12.
        crate::remove_instruction_retargeting_to_next(self, start + 11);
        let Instruction::RotateAndMaskRecord { s, .. } = &mut self.output.instructions[start + 11]
        else {
            unreachable!("the second bit test was matched")
        };
        *s = 3;
        let exit = match self.output.instructions[start + 13] {
            Instruction::Branch { target } => target,
            _ => unreachable!("the final guard exit was matched"),
        };
        self.output.instructions[start + 12] = Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: exit,
        };
        crate::remove_instruction_retargeting_to_next(self, start + 13);

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start + 13] else {
            unreachable!("the owner floor index was matched")
        };
        *d = 0;
        crate::insert_instruction_retargeting(self, start + 15, Instruction::move_register(23, 0));
        let Instruction::CompareWord { a, .. } = &mut self.output.instructions[start + 16] else {
            unreachable!("the floor-index comparison was matched")
        };
        *a = 0;
    }
}

fn allocated_object_collision_guard(instructions: &[Instruction]) -> Option<usize> {
    instructions
        .windows(17)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 24,
                    a: 29,
                    offset: 44,
                },
                Instruction::LoadByteZero {
                    d: 0,
                    a: 24,
                    offset: 8735,
                },
                Instruction::RotateAndMaskRecord {
                    a: 0,
                    s: 0,
                    shift: 28,
                    begin: 31,
                    end: 31,
                },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: first_exit,
                },
                Instruction::LoadWord { d: 0, a: 24, .. },
                Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: second_exit,
                },
                Instruction::LoadWord { d: 0, a: 24, .. },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: third_exit,
                },
                Instruction::LoadByteZero {
                    d: 0,
                    a: 24,
                    offset: 8735,
                },
                Instruction::RotateAndMaskRecord {
                    a: 0,
                    s: 0,
                    shift: 29,
                    begin: 31,
                    end: 31,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: body,
                },
                Instruction::Branch { target: fourth_exit },
                Instruction::LoadWord {
                    d: 23,
                    a: 30,
                    offset: floor_offset,
                },
                Instruction::LoadWord {
                    d: 25,
                    a: 24,
                    offset: peer_floor_offset,
                },
                Instruction::CompareWord { a: 23, b: 25 },
            ] if first_exit == second_exit
                && first_exit == third_exit
                && first_exit == fourth_exit
                && *body == start + 14
                && floor_offset == peer_floor_offset
            )
            .then_some(start)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_reloaded_object_collision_guard() {
        let exit = 80;
        let instructions = vec![
            Instruction::LoadWord {
                d: 24,
                a: 29,
                offset: 44,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 24,
                offset: 8735,
            },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 28,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: exit,
            },
            Instruction::LoadWord {
                d: 0,
                a: 24,
                offset: 224,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: exit,
            },
            Instruction::LoadWord {
                d: 0,
                a: 24,
                offset: 6744,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: exit,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 24,
                offset: 8735,
            },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 29,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::Branch { target: exit },
            Instruction::LoadWord {
                d: 23,
                a: 30,
                offset: 2108,
            },
            Instruction::LoadWord {
                d: 25,
                a: 24,
                offset: 2108,
            },
            Instruction::CompareWord { a: 23, b: 25 },
        ];

        assert_eq!(allocated_object_collision_guard(&instructions), Some(0));
    }
}
