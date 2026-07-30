//! Conditional status-call scheduling for object-state transfers.
//!
//! Metal-state calls fill receiver latency with independent member loads. The
//! following nested status guard retains one storage byte across both bit tests
//! and similarly delays each receiver until its final call-argument packet.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_status_schedule(
        &mut self,
        function: &Function,
    ) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        schedule_metal_status_calls(&mut self.output.instructions);
        let Some(packet) = allocated_nested_status_calls(&self.output.instructions) else {
            return;
        };

        crate::remove_instruction_retargeting_to_next(self, packet.start + 3);
        let original = self.output.instructions[packet.start..packet.start + 11].to_vec();
        let mut storage_load = original[0].clone();
        let mut outer_test = original[1].clone();
        let mut inner_test = original[3].clone();
        let Instruction::LoadByteZero { d, .. } = &mut storage_load else {
            unreachable!("the nested status storage load was matched")
        };
        *d = 3;
        for instruction in [&mut outer_test, &mut inner_test] {
            match instruction {
                Instruction::RotateAndMaskRecord { s, .. }
                | Instruction::ClearLeftImmediateRecord { s, .. } => *s = 3,
                _ => unreachable!("the nested status bit test was matched"),
            }
        }
        self.output.instructions[packet.start..packet.start + 11].clone_from_slice(&[
            storage_load,
            outer_test,
            original[2].clone(),
            inner_test,
            original[4].clone(),
            original[6].clone(),
            Instruction::AddImmediate {
                d: 3,
                a: 30,
                immediate: 0,
            },
            original[7].clone(),
            original[8].clone(),
            Instruction::move_register(3, 27),
            original[10].clone(),
        ]);
    }
}

fn schedule_metal_status_calls(instructions: &mut [Instruction]) {
    let Some(start) = allocated_metal_status_calls(instructions) else {
        return;
    };
    let original = instructions[start..start + 6].to_vec();
    instructions[start..start + 6].clone_from_slice(&[
        original[1].clone(),
        original[0].clone(),
        original[2].clone(),
        original[3].clone(),
        Instruction::move_register(3, 27),
        original[5].clone(),
    ]);
}

fn allocated_metal_status_calls(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::Or { a: 3, s: 30, b: 30 },
                Instruction::LoadWord { d: 4, a: 31, .. },
                Instruction::LoadWord { d: 5, a: 31, .. },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 27,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

#[derive(Clone, Copy)]
struct NestedStatusCalls {
    start: usize,
}

fn allocated_nested_status_calls(instructions: &[Instruction]) -> Option<NestedStatusCalls> {
    instructions
        .windows(12)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: outer_offset,
            }, Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 29,
                begin: 31,
                end: 31,
            }, Instruction::BranchConditionalForward { .. }, Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: inner_offset,
            }, Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: 31,
                end: 31,
            }, Instruction::BranchConditionalForward { .. }, Instruction::Or {
                a: 3,
                s: 30,
                b: 30,
            }, Instruction::LoadWord {
                d: 4,
                a: 31,
                ..
            }, Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 0,
            }, Instruction::BranchAndLink { .. }, Instruction::AddImmediate {
                d: 3,
                a: 27,
                immediate: 0,
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*outer_offset == *inner_offset).then_some(NestedStatusCalls { start })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_one_status_test_without_the_nested_call_packet() {
        assert!(allocated_nested_status_calls(&[
            Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: 8742,
            },
            Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 31,
            },
        ])
        .is_none());
    }
}
