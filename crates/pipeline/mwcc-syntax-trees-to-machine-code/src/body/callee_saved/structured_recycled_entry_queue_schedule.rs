//! Recycle expired entry-parameter homes in an inlined queue transaction.
//!
//! Three incoming values initially occupy `r29`-`r31`. After the two trailing
//! parameters are consumed by setup, build 163 reuses their homes for the
//! interrupt token and queue result instead of keeping the allocator's compact
//! block/result overlap.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_recycled_entry_queue_transaction(&mut self) -> bool {
        let Some(plan) = recycled_entry_queue_transaction(&self.output.instructions) else {
            return false;
        };

        let new_frame_size = plan.frame_size + 8;
        self.output.instructions[2] = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -new_frame_size,
        };
        self.output.instructions[3] = Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: new_frame_size - 4,
        };
        self.output.instructions[4] = Instruction::move_register(31, 4);
        self.output.instructions[5] = Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: new_frame_size - 8,
        };
        self.output.instructions[6] = Instruction::AddImmediate {
            d: 30,
            a: 5,
            immediate: 0,
        };
        self.output.instructions[7] = Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: new_frame_size - 12,
        };
        self.output.instructions[8] = Instruction::AddImmediate {
            d: 29,
            a: 3,
            immediate: 0,
        };

        let Instruction::LoadByteZero { a, .. } =
            &mut self.output.instructions[plan.trailing_member_load]
        else {
            unreachable!("the trailing entry member load was matched")
        };
        *a = 4;
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[plan.second_entry_store]
        else {
            unreachable!("the second entry store was matched")
        };
        *s = 31;
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[plan.third_entry_store]
        else {
            unreachable!("the third entry store was matched")
        };
        *s = 30;

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
        crate::move_instruction_before_retargeting(
            self,
            plan.disable_call + 5,
            plan.disable_call + 4,
        );
        self.output.instructions[plan.disable_call + 3] = Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0,
        };
        self.output.instructions[plan.disable_call + 4] = Instruction::AddImmediate {
            d: 4,
            a: 29,
            immediate: 0,
        };

        crate::move_instruction_before_retargeting(self, plan.queue_call + 2, plan.queue_call + 1);
        self.output.instructions[plan.queue_call + 2] = Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        };

        self.output.instructions[plan.restore_call - 1] = Instruction::move_register(3, 30);
        crate::move_instruction_before_retargeting(
            self,
            plan.restore_call + 2,
            plan.restore_call + 1,
        );
        self.output.instructions[plan.restore_call + 1] = Instruction::move_register(3, 31);

        for instruction in &mut self.output.instructions[plan.restore_call + 2..] {
            match instruction {
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == plan.frame_size + 4 => {
                    *offset = new_frame_size + 4
                }
                Instruction::LoadWord {
                    d: 31,
                    a: 1,
                    offset,
                } if *offset == plan.frame_size - 4 => *offset = new_frame_size - 4,
                Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset,
                } if *offset == plan.frame_size - 8 => *offset = new_frame_size - 8,
                Instruction::LoadWord {
                    d: 29,
                    a: 1,
                    offset,
                } if *offset == plan.frame_size - 12 => *offset = new_frame_size - 12,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == plan.frame_size => *immediate = new_frame_size,
                _ => {}
            }
        }
        self.frame_size = new_frame_size;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RecycledEntryQueueTransaction {
    frame_size: i16,
    trailing_member_load: usize,
    second_entry_store: usize,
    third_entry_store: usize,
    disable_call: usize,
    queue_call: usize,
    restore_call: usize,
}

fn recycled_entry_queue_transaction(
    instructions: &[Instruction],
) -> Option<RecycledEntryQueueTransaction> {
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
        }, Instruction::Or { a: 30, s: 4, b: 4 }, Instruction::Or { a: 31, s: 5, b: 5 }, Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: r29_offset,
        }, Instruction::Or { a: 29, s: 3, b: 3 }],
    ) = instructions.get(0..9)
    else {
        return None;
    };
    let frame_size = frame_update.checked_neg()?;
    if frame_size < 32
        || frame_size & 7 != 0
        || *r31_offset != frame_size - 4
        || *r30_offset != frame_size - 8
        || *r29_offset != frame_size - 12
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
                    d: 31,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 2,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 29,
                    offset: 12,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 2,
                },
                Instruction::Or { a: 4, s: 29, b: 29 },
            ])
        )
        || !matches!(
            instructions.get(queue_call + 1..queue_call + 3),
            Some([
                Instruction::AddImmediate {
                    d: 29,
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
                Instruction::Or { a: 3, s: 31, b: 31 },
                Instruction::BranchAndLink { .. },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset,
                },
                Instruction::Or { a: 3, s: 29, b: 29 },
            ]) if *offset == frame_size + 4
        )
    {
        return None;
    }

    let trailing_member_load = instructions[9..disable_call]
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::LoadByteZero {
                    d: 0,
                    a: 30,
                    offset: 4,
                }
            )
        })?
        + 9;
    let second_entry_store = instructions[trailing_member_load + 1..disable_call]
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s: 30,
                    a: 29,
                    offset: 36,
                }
            )
        })?
        + trailing_member_load
        + 1;
    let third_entry_store = instructions[second_entry_store + 1..disable_call]
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s: 31,
                    a: 29,
                    offset: 40,
                }
            )
        })?
        + second_entry_store
        + 1;

    Some(RecycledEntryQueueTransaction {
        frame_size,
        trailing_member_load,
        second_entry_store,
        third_entry_store,
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
    fn recognizes_three_entry_values_recycled_by_a_queue_transaction() {
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
                offset: -40,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 32,
            },
            Instruction::move_register(30, 4),
            Instruction::move_register(31, 5),
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 28,
            },
            Instruction::move_register(29, 3),
            Instruction::LoadByteZero {
                d: 0,
                a: 30,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 30,
                a: 29,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 31,
                a: 29,
                offset: 40,
            },
        ];
        instructions.extend([
            Instruction::BranchAndLink {
                target: "OSDisableInterrupts".into(),
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(0, 2),
            Instruction::StoreWord {
                s: 0,
                a: 29,
                offset: 12,
            },
            Instruction::load_immediate(3, 2),
            Instruction::move_register(4, 29),
            Instruction::BranchAndLink {
                target: "__DVDPushWaitingQueue".into(),
            },
            Instruction::AddImmediate {
                d: 29,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::move_register(3, 31),
            Instruction::BranchAndLink {
                target: "OSRestoreInterrupts".into(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::move_register(3, 29),
        ]);

        let plan = recycled_entry_queue_transaction(&instructions)
            .expect("the expired entry homes should be recyclable");
        assert_eq!(plan.frame_size, 40);
        assert_eq!(plan.trailing_member_load, 9);
        assert_eq!(plan.second_entry_store, 10);
        assert_eq!(plan.third_entry_store, 11);
        assert_eq!(plan.disable_call, 12);
        assert_eq!(plan.queue_call, 18);
        assert_eq!(plan.restore_call, 22);
    }
}
