//! Final ABI scheduling for a pointer posted to a global queue.
//!
//! Register allocation initially materializes the queue in r3 and the flag in
//! r5. MWCC instead gives the relocatable address r5 until its low half moves
//! into r3, using the independent flag materialization to fill the last call
//! latency slot. Build 163 applies the same value schedule inside its compact
//! linkage-first frame.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(crate) fn schedule_global_queue_pointer_send(&mut self) {
        let Some(shape) = global_queue_pointer_send(&self.output, self.behavior.frame_convention)
        else {
            return;
        };

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[4] else {
            unreachable!("the queue-address high instruction was matched")
        };
        *d = 5;
        let Instruction::AddImmediate { d, a, .. } = &mut self.output.instructions[6] else {
            unreachable!("the queue-address low instruction was matched")
        };
        *d = 3;
        *a = 5;

        match shape {
            QueueSendFrame::Predecrement => {
                crate::move_instruction_before_retargeting(self, 4, 2);
                crate::move_instruction_before_retargeting(self, 4, 3);
                crate::move_instruction_before_retargeting(self, 5, 4);
                crate::move_instruction_before_retargeting(self, 6, 5);
            }
            QueueSendFrame::LinkageFirst => {
                self.output.instructions[3] = Instruction::AddImmediate {
                    d: 4,
                    a: 3,
                    immediate: 0,
                };
                crate::move_instruction_before_retargeting(self, 4, 1);
                crate::move_instruction_before_retargeting(self, 3, 2);
                crate::move_instruction_before_retargeting(self, 4, 3);
                crate::move_instruction_before_retargeting(self, 6, 4);
                crate::move_instruction_before_retargeting(self, 6, 5);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueSendFrame {
    Predecrement,
    LinkageFirst,
}

fn global_queue_pointer_send(
    output: &mwcc_machine_code::MachineFunction,
    convention: FrameConvention,
) -> Option<QueueSendFrame> {
    let shape = match convention {
        FrameConvention::Predecrement
            if matches!(
                output.instructions.get(0..8),
                Some([
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -16,
                    },
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::AddImmediate { d: 5, a: 0, .. },
                    Instruction::Or { a: 4, s: 3, b: 3 },
                    Instruction::AddImmediateShifted {
                        d: 3,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: 1,
                        offset: 20,
                    },
                    Instruction::AddImmediate {
                        d: 3,
                        a: 3,
                        immediate: 0,
                    },
                    Instruction::BranchAndLink { .. },
                ])
            ) => QueueSendFrame::Predecrement,
        FrameConvention::LinkageFirst
            if matches!(
                output.instructions.get(0..8),
                Some([
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::AddImmediate { d: 5, a: 0, .. },
                    Instruction::StoreWord {
                        s: 0,
                        a: 1,
                        offset: 4,
                    },
                    Instruction::Or { a: 4, s: 3, b: 3 },
                    Instruction::AddImmediateShifted {
                        d: 3,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -8,
                    },
                    Instruction::AddImmediate {
                        d: 3,
                        a: 3,
                        immediate: 0,
                    },
                    Instruction::BranchAndLink { .. },
                ])
            ) => QueueSendFrame::LinkageFirst,
        _ => return None,
    };

    let high = external_relocation_target(output, 4, RelocationKind::Addr16Ha)?;
    let low = external_relocation_target(output, 6, RelocationKind::Addr16Lo)?;
    (high == low).then_some(shape)
}

fn external_relocation_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(target) => Some(target.as_str()),
            _ => None,
        }
    })
}
