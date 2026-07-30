//! Issue ordering for the three-home asynchronous callback switch.

use super::structured_async_callback_switch_layout::StructuredAsyncCallbackSwitchHomes;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_async_callback_switch(
        &mut self,
        homes: StructuredAsyncCallbackSwitchHomes,
    ) {
        self.schedule_async_callback_frame(homes);
        self.schedule_async_callback_switch_entry(homes);
        self.schedule_async_callback_calls(homes);
        self.schedule_async_callback_publication(homes.callback);
    }

    fn schedule_async_callback_frame(&mut self, homes: StructuredAsyncCallbackSwitchHomes) {
        if let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(window, [
                Instruction::StoreWord {
                    s: callback,
                    a: 1,
                    offset: callback_offset,
                },
                Instruction::Or {
                    a: copied,
                    s: 4,
                    b: 4,
                } | Instruction::AddImmediate {
                    d: copied,
                    a: 4,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: receiver,
                    a: 1,
                    offset: receiver_offset,
                },
                Instruction::Or {
                    a: copied_receiver,
                    s: 3,
                    b: 3,
                } | Instruction::AddImmediate {
                    d: copied_receiver,
                    a: 3,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: token,
                    a: 1,
                    offset: token_offset,
                },
            ] if *callback == homes.callback
                && *copied == homes.callback
                && *receiver == homes.receiver
                && *copied_receiver == homes.receiver
                && *token == homes.token
                && *callback_offset > *receiver_offset
                && *receiver_offset > *token_offset)
        }) {
            let callback_offset = match self.output.instructions[start] {
                Instruction::StoreWord { offset, .. } => offset,
                _ => unreachable!("the callback save was recognized"),
            };
            let receiver_offset = match self.output.instructions[start + 2] {
                Instruction::StoreWord { offset, .. } => offset,
                _ => unreachable!("the receiver save was recognized"),
            };
            let token_offset = match self.output.instructions[start + 4] {
                Instruction::StoreWord { offset, .. } => offset,
                _ => unreachable!("the token save was recognized"),
            };
            let callback_copy = self.output.instructions[start + 1].clone();
            let receiver_copy = self.output.instructions[start + 3].clone();
            self.output.instructions[start] = Instruction::StoreWord {
                s: homes.token,
                a: 1,
                offset: callback_offset,
            };
            self.output.instructions[start + 1] = Instruction::StoreWord {
                s: homes.callback,
                a: 1,
                offset: receiver_offset,
            };
            self.output.instructions[start + 2] = callback_copy;
            self.output.instructions[start + 3] = Instruction::StoreWord {
                s: homes.receiver,
                a: 1,
                offset: token_offset,
            };
            self.output.instructions[start + 4] = receiver_copy;
        }
    }

    fn schedule_async_callback_switch_entry(&mut self, homes: StructuredAsyncCallbackSwitchHomes) {
        let Some(start) = self.output.instructions.windows(3).position(|window| {
            matches!(window, [
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d, a: 3, immediate: 0 },
                Instruction::LoadWord { d: 4, a, .. },
            ] if *d == homes.token && *a == homes.receiver)
        }) else {
            return;
        };
        self.move_instruction_before(start + 2, start + 1);
    }

    fn schedule_async_callback_calls(&mut self, homes: StructuredAsyncCallbackSwitchHomes) {
        for start in 0..self.output.instructions.len().saturating_sub(4) {
            let matches_call = matches!(
                &self.output.instructions[start..start + 5],
                [
                    Instruction::AddImmediate {
                        d: 12,
                        a: callback,
                        immediate: 0,
                    },
                    Instruction::AddImmediate {
                        d: 3,
                        a: 0,
                        ..
                    },
                    Instruction::Or {
                        a: 4,
                        s: receiver,
                        b,
                    },
                    Instruction::MoveToLinkRegister { s: 12 },
                    Instruction::BranchToLinkRegisterAndLink,
                ] if *callback == homes.callback
                    && *receiver == homes.receiver
                    && *b == homes.receiver
            );
            if !matches_call {
                continue;
            }
            let first_argument = self.output.instructions[start + 1].clone();
            self.output.instructions[start + 1] = Instruction::MoveToLinkRegister { s: 12 };
            self.output.instructions[start + 2] = Instruction::AddImmediate {
                d: 4,
                a: homes.receiver,
                immediate: 0,
            };
            self.output.instructions[start + 3] = first_argument;
        }
    }

    fn schedule_async_callback_publication(&mut self, callback: u8) {
        let swaps: Vec<_> = self
            .output
            .instructions
            .windows(2)
            .enumerate()
            .filter_map(|(index, window)| {
                matches!(
                    window,
                    [
                        Instruction::StoreWord { s: 0, a: 0, .. },
                        Instruction::StoreWord { s, a: 0, .. },
                    ] if *s == callback
                )
                .then_some(index)
            })
            .collect();
        for index in swaps {
            self.move_instruction_before(index + 1, index);
        }
    }
}
