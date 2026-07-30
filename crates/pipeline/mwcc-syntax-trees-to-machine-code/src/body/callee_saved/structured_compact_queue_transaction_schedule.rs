//! Saved-home coloring and issue order for compact queued command transactions.
//!
//! Inlined two-home wrappers keep the command block and interrupt token in
//! `r31` and `r30`. Build 163 expands their frame to preserve its preferred
//! call schedule while publishing the command state in latency slots.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_compact_structured_queue_transaction(&mut self) {
        let Some(plan) = compact_structured_queue_transaction(&self.output.instructions) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, 6, 2);
        self.output.instructions[5] = Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        };

        let mut initialization_cursor = if matches!(
            self.output.instructions.get(7),
            Some(Instruction::StoreWord { s: 0, a: 31, .. })
        ) {
            8
        } else {
            7
        };
        if let Some((constant, member_offset)) = plan.initial_member_constant {
            let constant_index = self.output.instructions[initialization_cursor..plan.disable_call]
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::AddImmediate {
                            d: 0,
                            a: 0,
                            immediate,
                        } if *immediate == constant
                    )
                })
                .expect("the retained member constant was matched")
                + initialization_cursor;
            crate::move_instruction_before_retargeting(
                self,
                constant_index,
                initialization_cursor,
            );
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[initialization_cursor]
            else {
                unreachable!("the retained member constant was matched")
            };
            *d = 3;
            initialization_cursor += 1;

            let Instruction::StoreWord { a, .. } = &mut self.output.instructions[7] else {
                unreachable!("the command publication was matched")
            };
            *a = 3;
            let Instruction::StoreWord { s, .. } = self.output.instructions
                [initialization_cursor..plan.disable_call]
                .iter_mut()
                .find(|instruction| {
                    matches!(
                        instruction,
                        Instruction::StoreWord {
                            s: 0,
                            a: 31,
                            offset,
                        } if *offset == member_offset
                    )
                })
                .expect("the retained member constant store was matched")
            else {
                unreachable!("the retained member constant store was matched")
            };
            *s = 3;
        } else {
            for instruction in &mut self.output.instructions[7..plan.disable_call] {
                if let Instruction::StoreWord { a, .. } = instruction {
                    if *a == 31 {
                        *a = 3;
                    }
                }
            }
        }
        if let Some(zero_index) = self.output.instructions[initialization_cursor..plan.disable_call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 0,
                    }
                )
            })
            .map(|index| index + initialization_cursor)
        {
            crate::move_instruction_before_retargeting(self, zero_index, initialization_cursor);
        }

        crate::move_instruction_before_retargeting(
            self,
            plan.disable_call + 2,
            plan.disable_call + 1,
        );
        crate::move_instruction_before_retargeting(
            self,
            plan.disable_call + 3,
            plan.disable_call + 2,
        );
        crate::move_instruction_before_retargeting(self, plan.queue_call - 1, plan.queue_call - 2);
        self.output.instructions[plan.queue_call - 2] = Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: 0,
        };
        crate::move_instruction_before_retargeting(self, plan.queue_call + 2, plan.queue_call + 1);
        crate::move_instruction_before_retargeting(
            self,
            plan.restore_call + 2,
            plan.restore_call + 1,
        );

        let new_frame_size = plan.frame_size + 8;
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset }
                    if *offset == -plan.frame_size =>
                {
                    *offset = -new_frame_size
                }
                Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset,
                }
                | Instruction::LoadWord {
                    d: 31,
                    a: 1,
                    offset,
                } if *offset == plan.frame_size - 4 => *offset = new_frame_size - 4,
                Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset,
                }
                | Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset,
                } if *offset == plan.frame_size - 8 => *offset = new_frame_size - 8,
                Instruction::LoadWord { d: 0, a: 1, offset }
                    if *offset == plan.frame_size + 4 =>
                {
                    *offset = new_frame_size + 4
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == plan.frame_size => *immediate = new_frame_size,
                _ => {}
            }
        }
        self.frame_size = new_frame_size;
    }
}

#[derive(Clone, Copy)]
struct CompactStructuredQueueTransaction {
    frame_size: i16,
    initial_member_constant: Option<(i16, i16)>,
    disable_call: usize,
    queue_call: usize,
    restore_call: usize,
}

fn compact_structured_queue_transaction(
    instructions: &[Instruction],
) -> Option<CompactStructuredQueueTransaction> {
    let Some(
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: frame_update,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: r31_offset,
            },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: r30_offset,
            },
            Instruction::AddImmediate { d: 0, a: 0, .. },
        ],
    ) = instructions.get(0..7)
    else {
        return None;
    };
    let frame_size = frame_update.checked_neg()?;
    if frame_size < 24
        || frame_size & 7 != 0
        || *r31_offset != frame_size - 4
        || *r30_offset != frame_size - 8
    {
        return None;
    }

    let disable_call = call_index(instructions, "OSDisableInterrupts")?;
    let queue_call = call_index(instructions, "__DVDPushWaitingQueue")?;
    let restore_call = call_index(instructions, "OSRestoreInterrupts")?;
    let initial_member_constant = instructions[7..disable_call]
        .windows(2)
        .find_map(|window| match window {
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 31,
                    offset,
                },
            ] if *immediate != 0 => Some((*immediate, *offset)),
            _ => None,
        });
    if queue_call != disable_call + 6
        || !matches!(
            instructions.get(disable_call + 1..queue_call),
            Some([
                Instruction::AddImmediate {
                    d: 30,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 2,
                },
                Instruction::StoreWord { s: 0, a: 31, .. },
                Instruction::AddImmediate { d: 3, a: 0, .. },
                Instruction::Or { a: 4, s: 31, b: 31 },
            ])
        )
        || !matches!(
            instructions.get(queue_call + 1..queue_call + 3),
            Some([
                Instruction::AddImmediate {
                    d: 31,
                    a: 3,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
            ])
        )
        || !matches!(
            instructions.get(restore_call + 1..restore_call + 3),
            Some([
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::Or { a: 3, s: 31, b: 31 },
            ])
        )
    {
        return None;
    }
    Some(CompactStructuredQueueTransaction {
        frame_size,
        initial_member_constant,
        disable_call,
        queue_call,
        restore_call,
    })
}

fn call_index(instructions: &[Instruction], name: &str) -> Option<usize> {
    instructions.iter().position(
        |instruction| matches!(instruction, Instruction::BranchAndLink { target } if target == name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_compact_inlined_queue_transaction() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 16,
            },
            Instruction::load_immediate(0, 7),
            Instruction::BranchAndLink {
                target: "OSDisableInterrupts".into(),
            },
            Instruction::AddImmediate {
                d: 30,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(0, 2),
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 12,
            },
            Instruction::load_immediate(3, 1),
            Instruction::Or { a: 4, s: 31, b: 31 },
            Instruction::BranchAndLink {
                target: "__DVDPushWaitingQueue".into(),
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "OSRestoreInterrupts".into(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
        ];

        let plan = compact_structured_queue_transaction(&instructions)
            .expect("compact transaction should be recognized");
        assert_eq!(plan.frame_size, 24);
        assert_eq!(plan.initial_member_constant, None);
        assert_eq!(plan.disable_call, 7);
        assert_eq!(plan.queue_call, 13);
        assert_eq!(plan.restore_call, 16);

        instructions[2] = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -32,
        };
        instructions[3] = Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        };
        instructions[5] = Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        };
        instructions.splice(
            7..7,
            [
                Instruction::StoreWord {
                    s: 0,
                    a: 31,
                    offset: 8,
                },
                Instruction::StoreWord {
                    s: 4,
                    a: 31,
                    offset: 24,
                },
                Instruction::load_immediate(0, 32),
                Instruction::StoreWord {
                    s: 0,
                    a: 31,
                    offset: 20,
                },
            ],
        );
        let extended = compact_structured_queue_transaction(&instructions)
            .expect("extended initialization should use the same transaction");
        assert_eq!(extended.frame_size, 32);
        assert_eq!(extended.initial_member_constant, Some((32, 20)));
        assert_eq!(extended.disable_call, 11);
        assert_eq!(extended.queue_call, 17);
        assert_eq!(extended.restore_call, 20);
    }
}
