//! Physical scheduling for a writable-section anchor that is the frame's only
//! saved GPR.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InitialPartition {
    condition: usize,
    false_arm: usize,
}

impl Generator {
    pub(super) fn schedule_anchor_only_frame(&mut self) {
        // Build 163 starts materializing a retained writable-section base
        // between `mflr` and the linkage stores, then finishes it directly
        // into the sole saved register after that register has been saved.
        self.move_instruction_before(4, 1);
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[1] else {
            unreachable!("the anchor-only high half was matched")
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[5] else {
            unreachable!("the anchor-only low half was matched")
        };
        *a = 3;

        self.normalize_anchor_only_command_transactions();
        self.normalize_anchor_only_initial_partition();
    }

    fn normalize_anchor_only_command_transactions(&mut self) {
        if initial_partition(&self.output.instructions).is_none() {
            return;
        }
        if let Some(start) = first_anchored_member_call(&self.output.instructions) {
            self.move_instruction_before(start + 1, start);
        }
        if let Some(start) = anchored_invalidation_transaction(&self.output.instructions) {
            self.move_instruction_before(start + 3, start + 2);
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start] else {
                unreachable!("the invalidation receiver load was matched")
            };
            *d = 4;
            let Instruction::StoreWord { a, .. } = &mut self.output.instructions[start + 3] else {
                unreachable!("the invalidation receiver store was matched")
            };
            *a = 4;
        }
        while let Some(start) = callback_publication_transaction(&self.output.instructions) {
            self.move_instruction_before(start + 2, start + 1);
            let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[start]
            else {
                unreachable!("the callback high half was matched")
            };
            *d = 4;
            let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[start + 2]
            else {
                unreachable!("the callback low half was matched")
            };
            *a = 4;
        }
    }

    fn normalize_anchor_only_initial_partition(&mut self) {
        let Some(partition) = initial_partition(&self.output.instructions) else {
            return;
        };
        crate::insert_instruction_retargeting(
            self,
            partition.condition + 1,
            Instruction::Branch {
                target: partition.false_arm,
            },
        );
        let Instruction::LoadWord { d, .. } =
            &mut self.output.instructions[partition.condition - 2]
        else {
            unreachable!("the anchor-only partition load was matched")
        };
        *d = 0;
        let Instruction::CompareWordImmediate { a, .. } =
            &mut self.output.instructions[partition.condition - 1]
        else {
            unreachable!("the anchor-only partition comparison was matched")
        };
        *a = 0;
        let Instruction::BranchConditionalForward {
            options,
            condition_bit,
            target,
        } = &mut self.output.instructions[partition.condition]
        else {
            unreachable!("the anchor-only partition branch was matched")
        };
        *options = 12;
        *condition_bit = 2;
        *target = partition.condition + 2;
    }
}

pub(super) fn is_anchor_only_prefix(instructions: &[Instruction], frame_size: i16) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            ..
        ] if *offset == -frame_size
    )
}

fn initial_partition(instructions: &[Instruction]) -> Option<InitialPartition> {
    let [Instruction::LoadWord {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::CompareWordImmediate { a: 3, immediate }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: false_arm,
    }, ..] = instructions.get(6..)?
    else {
        return None;
    };
    if *immediate == 0 || *false_arm <= 9 || *false_arm >= instructions.len() {
        return None;
    }
    let true_arm = instructions.get(9..*false_arm)?;
    let false_tail = instructions.get(*false_arm..)?;
    (true_arm
        .iter()
        .filter(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .count()
        >= 3
        && true_arm
            .iter()
            .any(|instruction| matches!(instruction, Instruction::Branch { target } if *target > *false_arm))
        && false_tail
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. })))
    .then_some(InitialPartition {
        condition: 8,
        false_arm: *false_arm,
    })
}

fn first_anchored_member_call(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate { d: 3, a: 31, .. },
                Instruction::LoadWord {
                    d: 4,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 4,
                    offset: 36,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

fn anchored_invalidation_transaction(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 1,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 12,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 31,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: 32,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

fn callback_publication_transaction(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 3,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_calling_equality_partition_with_a_shared_exit() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::CompareWordImmediate { a: 3, immediate: 3 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 13,
            },
        ];
        instructions.extend((0..3).map(|index| Instruction::BranchAndLink {
            target: format!("call{index}"),
        }));
        instructions.push(Instruction::Branch { target: 15 });
        instructions.push(Instruction::BranchAndLink {
            target: "false_call".into(),
        });
        instructions.push(Instruction::Branch { target: 15 });
        instructions.push(Instruction::Branch { target: 15 });

        assert_eq!(
            initial_partition(&instructions),
            Some(InitialPartition {
                condition: 8,
                false_arm: 13,
            })
        );
    }
}
