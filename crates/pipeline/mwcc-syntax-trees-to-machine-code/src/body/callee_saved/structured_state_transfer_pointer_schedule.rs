//! Retained pointer scheduling for object-state transfers.
//!
//! Selection can allocate a guarded source member to r0, then reload the same
//! pointer for the taken path or its destination store.  The state-transfer
//! packet cannot mutate that member before either use, so MWCC retains the
//! original load across the null test.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_pointer_schedule(
        &mut self,
        function: &Function,
    ) {
        if !is_unused_array_state_transfer(function) {
            return;
        }

        while let Some(packet) = allocated_guarded_pointer_argument(&self.output.instructions) {
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[packet.load] else {
                unreachable!("the guarded pointer load was matched")
            };
            *d = Eabi::general_result().number;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[packet.load + 1]
            else {
                unreachable!("the guarded pointer comparison was matched")
            };
            *a = Eabi::general_result().number;
            crate::remove_instruction_retargeting_to_next(self, packet.reload);
        }

        while let Some(packet) = allocated_guarded_pointer_store(&self.output.instructions) {
            crate::remove_instruction_retargeting_to_next(self, packet.reload);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GuardedPointerPacket {
    load: usize,
    reload: usize,
}

fn allocated_guarded_pointer_argument(
    instructions: &[Instruction],
) -> Option<GuardedPointerPacket> {
    instructions
        .windows(4)
        .enumerate()
        .find_map(|(start, window)| match window {
            [
                Instruction::LoadWord {
                    d: 0,
                    a: 31,
                    offset: tested_offset,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: 0,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 31,
                    offset: argument_offset,
                },
            ] if tested_offset == argument_offset && *target > start + 4 => {
                Some(GuardedPointerPacket {
                    load: start,
                    reload: start + 3,
                })
            }
            _ => None,
        })
}

fn allocated_guarded_pointer_store(instructions: &[Instruction]) -> Option<GuardedPointerPacket> {
    instructions
        .windows(5)
        .enumerate()
        .find_map(|(start, window)| match window {
            [
                Instruction::LoadWord {
                    d: 0,
                    a: 31,
                    offset: tested_offset,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: 0,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 31,
                    offset: reload_offset,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 29,
                    offset: stored_offset,
                },
            ] if tested_offset == reload_offset
                && tested_offset == stored_offset
                && *target > start + 5 =>
            {
                Some(GuardedPointerPacket {
                    load: start,
                    reload: start + 3,
                })
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(target: usize) -> Instruction {
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target,
        }
    }

    #[test]
    fn recognizes_a_guarded_pointer_argument_reload() {
        let instructions = [
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6524,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            branch(8),
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 6524,
            },
        ];

        assert_eq!(
            allocated_guarded_pointer_argument(&instructions),
            Some(GuardedPointerPacket { load: 0, reload: 3 })
        );
    }

    #[test]
    fn recognizes_a_guarded_pointer_destination_store_reload() {
        let instructions = [
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6516,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            branch(8),
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6516,
            },
            Instruction::StoreWord {
                s: 0,
                a: 29,
                offset: 6516,
            },
        ];

        assert_eq!(
            allocated_guarded_pointer_store(&instructions),
            Some(GuardedPointerPacket { load: 0, reload: 3 })
        );
    }

    #[test]
    fn rejects_a_reload_from_an_unrelated_member() {
        let mut instructions = [
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6524,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            branch(8),
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 6528,
            },
        ];

        assert_eq!(allocated_guarded_pointer_argument(&instructions), None);
        instructions[3] = Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: 6524,
        };
        assert_eq!(allocated_guarded_pointer_argument(&instructions), None);
    }
}
