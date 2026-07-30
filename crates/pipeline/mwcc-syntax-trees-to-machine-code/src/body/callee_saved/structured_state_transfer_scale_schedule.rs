//! Scale-call preparation for object-state transfers.
//!
//! Two adjacent bit-field stores and a scalar copy precede the scale call.
//! MWCC reserves r3 for the retained destination object after the first source
//! byte load, carries both inserted fields in r4, and fills the remaining call
//! latency with the independent stores.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_scale_schedule(
        &mut self,
        function: &Function,
    ) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(packet) = allocated_state_transfer_scale_packet(&self.output.instructions) else {
            return;
        };

        let original = self.output.instructions[packet.start..packet.start + 13].to_vec();
        let mut first_source = original[0].clone();
        let mut first_insert = original[2].clone();
        let mut second_source = original[4].clone();
        let mut second_insert = original[6].clone();
        for instruction in [&mut first_source, &mut second_source] {
            let Instruction::LoadByteZero { d, .. } = instruction else {
                unreachable!("the source bit-field load was matched")
            };
            *d = 4;
        }
        for instruction in [&mut first_insert, &mut second_insert] {
            let Instruction::RotateAndMaskInsert { s, .. } = instruction else {
                unreachable!("the source bit-field insert was matched")
            };
            *s = 4;
        }
        self.output.instructions[packet.start..packet.start + 13].clone_from_slice(&[
            first_source,
            Instruction::move_register(3, 30),
            original[1].clone(),
            first_insert,
            original[3].clone(),
            second_source,
            original[5].clone(),
            second_insert,
            original[7].clone(),
            original[8].clone(),
            original[9].clone(),
            original[11].clone(),
            original[12].clone(),
        ]);
    }
}

#[derive(Clone, Copy)]
struct StateTransferScalePacket {
    start: usize,
}

fn allocated_state_transfer_scale_packet(
    instructions: &[Instruction],
) -> Option<StateTransferScalePacket> {
    instructions
        .windows(13)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: first_source_offset,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: first_destination_offset,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: 0,
                begin: 29,
                end: 29,
            }, Instruction::StoreByte {
                s: 0,
                a: 29,
                offset: first_store_offset,
            }, Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: second_source_offset,
            }, Instruction::LoadByteZero {
                d: 0,
                a: 29,
                offset: second_destination_offset,
            }, Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: 0,
                begin: 30,
                end: 30,
            }, Instruction::StoreByte {
                s: 0,
                a: 29,
                offset: second_store_offset,
            }, Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: word_source_offset,
            }, Instruction::StoreWord {
                s: 0,
                a: 29,
                offset: word_destination_offset,
            }, Instruction::AddImmediate {
                d: 3,
                a: 30,
                immediate: 0,
            }, Instruction::LoadFloatSingle {
                d: 1,
                a: 31,
                ..
            }, Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*first_source_offset == *first_destination_offset
                && *first_source_offset == *first_store_offset
                && *second_source_offset == *second_destination_offset
                && *second_source_offset == *second_store_offset
                && *word_source_offset == *word_destination_offset)
                .then_some(StateTransferScalePacket { start })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_incomplete_scale_packet() {
        assert!(allocated_state_transfer_scale_packet(&[
            Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: 8736,
            },
            Instruction::BranchAndLink {
                target: "scale".into(),
            },
        ])
        .is_none());
    }
}
