//! Floating HP argument scheduling for object-state transfers.
//!
//! The source body retains a dead automatic array and a temporary bit lane.
//! MWCC keeps those optimizer-owned lanes below the conversion image even
//! though neither produces an explicit access. The floating load starts the
//! call packet, independent byte arguments fill its conversion latency, and
//! the `fctiwz` result lands above those retained lanes.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

const RETAINED_LOWER_FRAME_BYTES: i16 = 24;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_hp_call(&mut self, function: &Function) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(packet) = allocated_state_transfer_hp_call(&self.output.instructions) else {
            return;
        };

        let scratch = packet.scratch.saturating_add(RETAINED_LOWER_FRAME_BYTES);
        self.output.instructions[packet.start..packet.start + 8].clone_from_slice(&[
            Instruction::LoadFloatSingle {
                d: 0,
                a: 29,
                offset: packet.hp_offset,
            },
            Instruction::LoadByteZero {
                d: 4,
                a: 29,
                offset: packet.bitfield_offset,
            },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::LoadByteZero {
                d: 3,
                a: 29,
                offset: packet.player_offset,
            },
            Instruction::RotateAndMask {
                a: 4,
                s: 4,
                shift: 29,
                begin: 31,
                end: 31,
            },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: scratch,
            },
            Instruction::LoadWord {
                d: 5,
                a: 1,
                offset: scratch.saturating_add(4),
            },
            packet.call,
        ]);
    }
}

#[derive(Clone)]
struct StateTransferHpCall {
    start: usize,
    player_offset: i16,
    bitfield_offset: i16,
    hp_offset: i16,
    scratch: i16,
    call: Instruction,
}

fn allocated_state_transfer_hp_call(instructions: &[Instruction]) -> Option<StateTransferHpCall> {
    instructions
        .windows(8)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadByteZero {
                d: 3,
                a: 29,
                offset: player_offset,
            }, Instruction::LoadByteZero {
                d: 4,
                a: 29,
                offset: bitfield_offset,
            }, Instruction::RotateAndMask {
                a: 4,
                s: 4,
                shift: 29,
                begin: 31,
                end: 31,
            }, Instruction::LoadFloatSingle {
                d: 0,
                a: 29,
                offset: hp_offset,
            }, Instruction::ConvertToIntegerWordZero { d: 0, b: 0 }, Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: scratch,
            }, Instruction::LoadWord {
                d: 5,
                a: 1,
                offset: scratch_word,
            }, call @ Instruction::BranchAndLink { .. }] = window
            else {
                return None;
            };
            (*scratch_word == scratch.saturating_add(4)).then(|| StateTransferHpCall {
                start,
                player_offset: *player_offset,
                bitfield_offset: *bitfield_offset,
                hp_offset: *hp_offset,
                scratch: *scratch,
                call: call.clone(),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_conversion_without_the_complete_call_packet() {
        assert!(allocated_state_transfer_hp_call(&[
            Instruction::LoadFloatSingle {
                d: 0,
                a: 29,
                offset: 6192,
            },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        ])
        .is_none());
    }
}
