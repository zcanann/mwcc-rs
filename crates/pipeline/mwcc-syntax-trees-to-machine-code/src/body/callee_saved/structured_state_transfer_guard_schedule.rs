//! Guarded collision scheduling for object-state transfers.
//!
//! The first collision arm ends with a saved-object call. The following
//! assignment condition retains its tested member in the argument register,
//! avoiding a reload when the guarded callback consumes that same value.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_guard_schedule(
        &mut self,
        function: &Function,
    ) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(packet) = allocated_state_transfer_guard_packet(&self.output.instructions) else {
            return;
        };

        crate::remove_instruction_retargeting_to_next(self, packet.start + 18);
        self.output.instructions[packet.start + 4] = Instruction::move_register(3, 31);
        self.output.instructions[packet.start + 11] =
            Instruction::CompareWordImmediate { a: 0, immediate: 0 };
        self.output.instructions[packet.start + 12] = Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: packet.assignment_offset,
        };
        self.output.instructions[packet.start + 14] = Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: packet.tested_offset,
        };
        self.output.instructions[packet.start + 15] =
            Instruction::CompareWordImmediate { a: 4, immediate: 0 };
        self.output.instructions[packet.start + 17] = Instruction::move_register(3, 30);
    }
}

#[derive(Clone, Copy)]
struct StateTransferGuardPacket {
    start: usize,
    assignment_offset: i16,
    tested_offset: i16,
}

fn allocated_state_transfer_guard_packet(
    instructions: &[Instruction],
) -> Option<StateTransferGuardPacket> {
    instructions
        .windows(20)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0,
            }, Instruction::AddImmediate {
                d: 4,
                a: 29,
                immediate: 0,
            }, Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 107,
            }, Instruction::BranchAndLink { .. }, Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0,
            }, Instruction::BranchAndLink { .. }, Instruction::Branch { .. }, Instruction::AddImmediate {
                d: 3,
                a: 29,
                immediate: 0,
            }, Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 9,
            }, Instruction::BranchAndLink { .. }, Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 2,
            }, Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: assignment_offset,
            }, Instruction::CompareWordImmediate {
                a: 0,
                immediate: 0,
            }, Instruction::BranchConditionalForward { .. }, Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: tested_offset,
            }, Instruction::CompareWordImmediate {
                a: 0,
                immediate: 0,
            }, Instruction::BranchConditionalForward { .. }, Instruction::Or {
                a: 3,
                s: 30,
                b: 30,
            }, Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: reload_offset,
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*tested_offset == *reload_offset).then_some(StateTransferGuardPacket {
                start,
                assignment_offset: *assignment_offset,
                tested_offset: *tested_offset,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_incomplete_assignment_guard() {
        assert!(allocated_state_transfer_guard_packet(&[
            Instruction::load_immediate(0, 2),
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ])
        .is_none());
    }
}
