//! Saved-home coloring and issue order for queued command transactions.
//!
//! Two entry values survive an optional preparation call. A later interrupt
//! token survives the queue call, while the queue result reuses the expired
//! command-block home. Build 163 colors those roles `r29`, `r30`, and `r31`
//! respectively and fills call latency slots with the state publication.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_queue_transaction(&mut self, function: &Function) {
        if self.schedule_recycled_entry_queue_transaction() {
            return;
        }
        let Some(plan) = structured_queue_transaction(&self.output.instructions) else {
            if function.return_type == Type::Void {
                self.schedule_void_queue_transaction();
            } else {
                self.schedule_compact_structured_queue_transaction();
            }
            return;
        };

        crate::move_instruction_before_retargeting(self, 5, 4);
        crate::move_instruction_before_retargeting(self, 7, 6);
        self.output.instructions[5] = Instruction::AddImmediate {
            d: 30,
            a: 4,
            immediate: 0,
        };
        self.output.instructions[7] = Instruction::AddImmediate {
            d: 29,
            a: 3,
            immediate: 0,
        };

        for instruction in &mut self.output.instructions[8..plan.disable_call + 4] {
            match instruction {
                Instruction::LoadWord { a, .. } | Instruction::StoreWord { a, .. } if *a == 31 => {
                    *a = 30
                }
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
        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.disable_call + 3]
        else {
            unreachable!("the interrupt token copy was matched")
        };
        *d = 31;
        self.output.instructions[plan.disable_call + 4] = Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        };
        self.output.instructions[plan.disable_call + 5] = Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        };

        crate::move_instruction_before_retargeting(self, plan.queue_call + 2, plan.queue_call + 1);
        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.queue_call + 2]
        else {
            unreachable!("the queue result copy was matched")
        };
        *d = 30;

        let Instruction::Or { s, b, .. } = &mut self.output.instructions[plan.restore_argument]
        else {
            unreachable!("the interrupt restore argument was matched")
        };
        *s = 31;
        *b = 31;
        let Instruction::Or { s, b, .. } = &mut self.output.instructions[plan.return_copy] else {
            unreachable!("the queue result return was matched")
        };
        *s = 30;
        *b = 30;
    }
}

#[derive(Clone, Copy)]
struct StructuredQueueTransaction {
    disable_call: usize,
    queue_call: usize,
    restore_argument: usize,
    return_copy: usize,
}

fn structured_queue_transaction(
    instructions: &[Instruction],
) -> Option<StructuredQueueTransaction> {
    if !matches!(
        instructions.get(0..8),
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
                offset: -32,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::Or { a: 30, s: 3, b: 3 },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 20,
            },
        ])
    ) {
        return None;
    }

    let start = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 29,
                    a: 3,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 2,
                },
                Instruction::StoreWord { s: 0, a: 31, .. },
                Instruction::Or { a: 3, s: 30, b: 30 },
                Instruction::Or { a: 4, s: 31, b: 31 },
                Instruction::BranchAndLink { .. },
            ]
        )
    })?;
    let disable_call = start.checked_sub(1)?;
    if !matches!(
        instructions[disable_call],
        Instruction::BranchAndLink { .. }
    ) {
        return None;
    }
    let queue_call = start + 5;
    if !matches!(
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
    ) {
        return None;
    }
    let restore_argument = instructions[queue_call + 3..]
        .windows(4)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::Or { a: 3, s: 29, b: 29 },
                    Instruction::BranchAndLink { .. },
                    Instruction::Or { a: 3, s: 31, b: 31 },
                    Instruction::LoadWord { d: 0, a: 1, .. },
                ]
            )
        })?
        + queue_call
        + 3;
    Some(StructuredQueueTransaction {
        disable_call,
        queue_call,
        restore_argument,
        return_copy: restore_argument + 2,
    })
}
