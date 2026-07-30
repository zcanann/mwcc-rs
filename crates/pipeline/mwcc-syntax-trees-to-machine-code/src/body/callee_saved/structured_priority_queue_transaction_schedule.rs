//! Saved-home coloring for queued command wrappers with a dynamic priority.
//!
//! The command block, interrupt token, priority, and queue result overlap in a
//! way that needs three saved homes. Build 163 uses `r29` for the block, `r30`
//! for the interrupt token, and `r31` for both priority and the later result.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_priority_queue_transaction(&mut self) {
        let Some(plan) = priority_queue_transaction(&self.output.instructions) else {
            return;
        };

        let new_frame_size = plan.frame_size + 16;
        crate::move_instruction_before_retargeting(self, 7, 2);
        crate::move_instruction_before_retargeting(self, 7, 5);
        if matches!(
            self.output.instructions[5],
            Instruction::Or { a: 31, s: 8, b: 8 }
        ) {
            self.output.instructions[5] = Instruction::AddImmediate {
                d: 31,
                a: 8,
                immediate: 0,
            };
        }
        crate::insert_instruction_retargeting(
            self,
            7,
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: new_frame_size - 12,
            },
        );
        self.output.instructions[8] = Instruction::AddImmediate {
            d: 29,
            a: 3,
            immediate: 0,
        };

        let disable_call = plan.disable_call + 1;
        let queue_call = plan.queue_call + 1;
        let restore_call = plan.restore_call + 1;
        for instruction in &mut self.output.instructions[9..disable_call] {
            match instruction {
                Instruction::LoadWord { a, .. } if *a == 30 => *a = 29,
                Instruction::StoreWord { a, .. } if *a == 30 => *a = 3,
                _ => {}
            }
        }
        if let Some(zero_index) = self.output.instructions[10..disable_call]
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
            .map(|index| index + 10)
        {
            crate::move_instruction_before_retargeting(self, zero_index, 10);
        }

        crate::move_instruction_before_retargeting(self, disable_call + 2, disable_call + 1);
        crate::move_instruction_before_retargeting(self, disable_call + 3, disable_call + 2);
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[disable_call + 2]
        else {
            unreachable!("the command status store was matched")
        };
        *a = 29;
        self.output.instructions[disable_call + 4] = Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        };
        self.output.instructions[disable_call + 5] = Instruction::AddImmediate {
            d: 4,
            a: 29,
            immediate: 0,
        };

        crate::move_instruction_before_retargeting(self, queue_call + 2, queue_call + 1);
        crate::move_instruction_before_retargeting(self, restore_call + 2, restore_call + 1);

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
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == plan.frame_size + 4 => {
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
        let stack_restore = self
            .output
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate,
                    } if *immediate == new_frame_size
                )
            })
            .expect("the stack restore was matched");
        crate::insert_instruction_retargeting(
            self,
            stack_restore,
            Instruction::LoadWord {
                d: 29,
                a: 1,
                offset: new_frame_size - 12,
            },
        );
        self.frame_size = new_frame_size;
    }
}

#[derive(Clone, Copy)]
struct PriorityQueueTransaction {
    frame_size: i16,
    disable_call: usize,
    queue_call: usize,
    restore_call: usize,
}

fn priority_queue_transaction(instructions: &[Instruction]) -> Option<PriorityQueueTransaction> {
    let Some(
        [Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        }, Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: frame_update,
        }, Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: r31_offset,
        }, Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: r30_offset,
        }, Instruction::Or { a: 30, s: 3, b: 3 }, Instruction::Or { a: 31, .. }, Instruction::AddImmediate { d: 0, a: 0, .. }],
    ) = instructions.get(0..8)
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
                Instruction::StoreWord { s: 0, a: 30, .. },
                Instruction::Or { a: 3, s: 31, b: 31 },
                Instruction::Or { a: 4, s: 30, b: 30 },
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
            instructions.get(restore_call - 1..restore_call + 3),
            Some([
                Instruction::Or { a: 3, s: 30, b: 30 },
                Instruction::BranchAndLink { .. },
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::Or { a: 3, s: 31, b: 31 },
            ])
        )
    {
        return None;
    }
    Some(PriorityQueueTransaction {
        frame_size,
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
    fn recognizes_a_dynamic_priority_queue_transaction() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::Or { a: 30, s: 3, b: 3 },
            Instruction::Or { a: 31, s: 6, b: 6 },
            Instruction::load_immediate(0, 2),
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
                a: 30,
                offset: 12,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::Or { a: 4, s: 30, b: 30 },
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
            Instruction::Or { a: 3, s: 30, b: 30 },
            Instruction::BranchAndLink {
                target: "OSRestoreInterrupts".into(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
        ];

        let plan = priority_queue_transaction(&instructions)
            .expect("dynamic-priority transaction should be recognized");
        assert_eq!(plan.frame_size, 32);
        assert_eq!(plan.disable_call, 8);
        assert_eq!(plan.queue_call, 14);
        assert_eq!(plan.restore_call, 18);
    }
}
