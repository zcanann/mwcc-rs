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
        if let Some(
            [Instruction::StoreWord {
                a: command_base, ..
            }, Instruction::StoreWord {
                a: callback_base, ..
            }],
        ) = self.output.instructions.get_mut(7..9)
        {
            *command_base = 3;
            *callback_base = 3;
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

        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } if *offset == -24 => {
                    *offset = -32
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
                } if *offset == 20 => *offset = 28,
                Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset,
                }
                | Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset,
                } if *offset == 16 => *offset = 24,
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == 28 => *offset = 36,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == 24 => *immediate = 32,
                _ => {}
            }
        }
        self.frame_size = 32;
    }
}

#[derive(Clone, Copy)]
struct CompactStructuredQueueTransaction {
    disable_call: usize,
    queue_call: usize,
    restore_call: usize,
}

fn compact_structured_queue_transaction(
    instructions: &[Instruction],
) -> Option<CompactStructuredQueueTransaction> {
    if !matches!(
        instructions.get(0..7),
        Some([
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
            Instruction::AddImmediate { d: 0, a: 0, .. },
        ])
    ) {
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
        assert_eq!(plan.disable_call, 7);
        assert_eq!(plan.queue_call, 13);
        assert_eq!(plan.restore_call, 16);
    }
}
