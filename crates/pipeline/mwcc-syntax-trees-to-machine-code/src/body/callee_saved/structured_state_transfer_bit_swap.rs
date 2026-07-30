//! Bitfield lifetime scheduling for object-state transfers.
//!
//! The source bit remains in r5 while the destination storage byte is extracted
//! through r4. Keeping those two values distinct removes a redundant extraction
//! and lets the destination payload serve as the following call receiver.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_bit_swap(&mut self, function: &Function) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let (start, storage_offset, has_redundant_receiver_move) =
            if let Some((start, storage_offset)) =
                allocated_state_transfer_bit_swap(&self.output.instructions)
            {
                (start, storage_offset, true)
            } else if let Some((start, storage_offset)) =
                allocated_compact_state_transfer_bit_swap(&self.output.instructions)
            {
                (start, storage_offset, false)
            } else {
                return;
            };

        if has_redundant_receiver_move {
            self.move_instruction_before(start + 11, start + 10);
            crate::remove_instruction_retargeting_to_next(self, start + 11);
        };

        self.output.instructions[start..start + 10].clone_from_slice(&[
            Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: storage_offset,
            },
            Instruction::move_register(3, 29),
            Instruction::LoadByteZero {
                d: 5,
                a: 31,
                offset: storage_offset,
            },
            Instruction::RotateAndMask {
                a: 4,
                s: 0,
                shift: 29,
                begin: 31,
                end: 31,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: storage_offset,
            },
            Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 3,
                begin: 28,
                end: 28,
            },
            Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: storage_offset,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: storage_offset,
            },
            Instruction::RotateAndMaskInsert {
                a: 0,
                s: 5,
                shift: 0,
                begin: 28,
                end: 28,
            },
            Instruction::StoreByte {
                s: 0,
                a: 29,
                offset: storage_offset,
            },
        ]);
    }
}

fn allocated_state_transfer_bit_swap(instructions: &[Instruction]) -> Option<(usize, i16)> {
    instructions
        .windows(12)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: source_offset,
            }, Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 29,
                begin: 31,
                end: 31,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: source_storage_offset,
            }, Instruction::LoadByteZero {
                d: 4,
                a: 29,
                offset: destination_offset,
            }, Instruction::RotateAndMask {
                a: 4,
                s: 4,
                shift: 29,
                begin: 31,
                end: 31,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 3,
                begin: 28,
                end: 28,
            }, Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: source_store_offset,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: destination_storage_offset,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: 3,
                begin: 28,
                end: 28,
            }, Instruction::StoreByte {
                s: 0,
                a: 29,
                offset: destination_store_offset,
            }, Instruction::AddImmediate {
                d: 3,
                a: 29,
                immediate: 0,
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*source_offset == *source_storage_offset
                && *source_offset == *destination_offset
                && *source_offset == *source_store_offset
                && *source_offset == *destination_storage_offset
                && *source_offset == *destination_store_offset)
                .then_some((start, *source_offset))
        })
}

fn allocated_compact_state_transfer_bit_swap(
    instructions: &[Instruction],
) -> Option<(usize, i16)> {
    instructions
        .windows(11)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: source_offset,
            }, Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 29,
                begin: 31,
                end: 31,
            }, Instruction::LoadByteZero {
                d: 4,
                a: 29,
                offset: destination_source_offset,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: source_storage_offset,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 0,
                begin: 28,
                end: 28,
            }, Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: source_store_offset,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: destination_storage_offset,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: 3,
                begin: 28,
                end: 28,
            }, Instruction::StoreByte {
                s: 0,
                a: 29,
                offset: destination_store_offset,
            }, Instruction::AddImmediate {
                d: 3,
                a: 29,
                immediate: 0,
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*source_offset == *destination_source_offset
                && *source_offset == *source_storage_offset
                && *source_offset == *source_store_offset
                && *source_offset == *destination_storage_offset
                && *source_offset == *destination_store_offset)
                .then_some((start, *source_offset))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_short_unrelated_bitfield_packet() {
        assert_eq!(
            allocated_state_transfer_bit_swap(&[
                Instruction::LoadByteZero {
                    d: 3,
                    a: 31,
                    offset: 8735,
                },
                Instruction::RotateAndMask {
                    a: 3,
                    s: 3,
                    shift: 29,
                    begin: 31,
                    end: 31,
                },
            ]),
            None
        );
    }
}
