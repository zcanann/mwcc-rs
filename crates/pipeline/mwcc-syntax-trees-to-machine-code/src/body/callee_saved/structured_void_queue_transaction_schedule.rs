//! Saved-home coloring for queued command wrappers that discard the result.
//!
//! A void wrapper needs only the command block and interrupt token after the
//! queue call. Build 163 colors them `r30` and `r31`, respectively, and omits
//! the returning wrapper's queue-result copy.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_void_queue_transaction(&mut self) {
        let Some(plan) = void_queue_transaction(&self.output.instructions) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, 6, 2);
        crate::move_instruction_before_retargeting(self, 6, 5);
        self.output.instructions[6] = Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0,
        };
        for instruction in &mut self.output.instructions[7..plan.disable_call] {
            match instruction {
                Instruction::LoadWord { a, .. } if *a == 31 => *a = 30,
                Instruction::StoreWord { a, .. } if *a == 31 => *a = 3,
                _ => {}
            }
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
        let Instruction::StoreWord { a, .. } =
            &mut self.output.instructions[plan.disable_call + 2]
        else {
            unreachable!("the command status store was matched")
        };
        *a = 30;
        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.disable_call + 3]
        else {
            unreachable!("the interrupt token copy was matched")
        };
        *d = 31;
        crate::move_instruction_before_retargeting(self, plan.queue_call - 1, plan.queue_call - 2);
        self.output.instructions[plan.queue_call - 2] = Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        };

        crate::remove_instruction_retargeting_to_next(self, plan.queue_call + 1);
        let restore_call = plan.restore_call - 1;
        let Instruction::Or { s, b, .. } = &mut self.output.instructions[restore_call - 1] else {
            unreachable!("the interrupt restore argument was matched")
        };
        *s = 31;
        *b = 31;
        crate::remove_instruction_retargeting_to_next(self, restore_call + 2);

        let new_frame_size = plan.frame_size + 16;
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
        self.frame_size = new_frame_size;
    }
}

#[derive(Clone, Copy)]
struct VoidQueueTransaction {
    frame_size: i16,
    disable_call: usize,
    queue_call: usize,
    restore_call: usize,
}

fn void_queue_transaction(instructions: &[Instruction]) -> Option<VoidQueueTransaction> {
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
        }, Instruction::Or { a: 31, s: 3, b: 3 }, Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: r30_offset,
        }, Instruction::AddImmediate { d: 0, a: 0, .. }],
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
    Some(VoidQueueTransaction {
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
    fn recognizes_a_void_queue_transaction() {
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
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::load_immediate(0, 13),
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
            Instruction::load_immediate(3, 2),
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

        let plan =
            void_queue_transaction(&instructions).expect("void transaction should be recognized");
        assert_eq!(plan.frame_size, 32);
        assert_eq!(plan.disable_call, 7);
        assert_eq!(plan.queue_call, 13);
        assert_eq!(plan.restore_call, 17);
    }
}
