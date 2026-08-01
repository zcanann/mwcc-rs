//! Alternating byte-store scheduling after a translation-unit assembly barrier.
//!
//! Build 163 keeps the two repeated byte constants in separate volatile
//! registers, expands the intervening relocated global load into an explicit
//! address, and narrows that load before issuing the store run. Keeping this
//! physical packet here isolates the assembly-barrier policy from ordinary
//! structured constant and global-address selection.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_asm_barrier_byte_stores(&mut self) -> bool {
        if !self.preceded_by_asm
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_callee_saved_frame_layout
                != LegacyCalleeSavedFrameLayout::CompactValueHomes
        {
            return false;
        }
        let Some(packet) = alternating_byte_store_packet(
            &self.output.instructions,
            &self.output.relocations,
        ) else {
            return false;
        };
        rewrite_alternating_byte_store_packet(&mut self.output.instructions, packet);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AlternatingByteStorePacket {
    start: usize,
    object: u8,
    first_offset: i16,
    first_constant: i16,
    second_constant: i16,
}

fn rewrite_alternating_byte_store_packet(
    instructions: &mut Vec<Instruction>,
    packet: AlternatingByteStorePacket,
) {
    let start = packet.start;
    instructions.splice(
        start..start + 12,
        [
            Instruction::load_immediate_shifted(3, 0),
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: 0,
            },
            Instruction::load_immediate(4, packet.first_constant),
            Instruction::load_immediate(0, packet.second_constant),
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 24,
                end: 31,
            },
            Instruction::StoreByte {
                s: 3,
                a: packet.object,
                offset: packet.first_offset,
            },
            Instruction::load_immediate(3, 0),
            Instruction::StoreByte {
                s: 4,
                a: packet.object,
                offset: packet.first_offset + 1,
            },
            Instruction::StoreByte {
                s: 0,
                a: packet.object,
                offset: packet.first_offset + 2,
            },
            Instruction::StoreByte {
                s: 4,
                a: packet.object,
                offset: packet.first_offset + 3,
            },
            Instruction::StoreByte {
                s: 0,
                a: packet.object,
                offset: packet.first_offset + 4,
            },
        ],
    );
}

fn alternating_byte_store_packet(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<AlternatingByteStorePacket> {
    instructions.windows(13).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediateShifted { d: address, a: 0, .. },
            Instruction::LoadWord { d: 0, a: load_base, .. },
            Instruction::StoreByte { s: 0, a: object, offset: first_offset },
            Instruction::AddImmediate { d: 0, a: 0, immediate: first_constant },
            Instruction::StoreByte { s: 0, a: first_store_base, offset: second_offset },
            Instruction::AddImmediate { d: 0, a: 0, immediate: second_constant },
            Instruction::StoreByte { s: 0, a: second_store_base, offset: third_offset },
            Instruction::AddImmediate { d: 0, a: 0, immediate: repeated_first },
            Instruction::StoreByte { s: 0, a: third_store_base, offset: fourth_offset },
            Instruction::AddImmediate { d: 0, a: 0, immediate: repeated_second },
            Instruction::StoreByte { s: 0, a: fourth_store_base, offset: fifth_offset },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            _,
        ] = window
        else {
            return None;
        };
        if address != load_base
            || first_constant == second_constant
            || first_constant != repeated_first
            || second_constant != repeated_second
            || object != first_store_base
            || object != second_store_base
            || object != third_store_base
            || object != fourth_store_base
            || second_offset != &(first_offset + 1)
            || third_offset != &(first_offset + 2)
            || fourth_offset != &(first_offset + 3)
            || fifth_offset != &(first_offset + 4)
        {
            return None;
        }

        let high = relocations.iter().find(|relocation| {
            relocation.instruction_index == start
                && relocation.kind == RelocationKind::Addr16Ha
        })?;
        let low = relocations.iter().find(|relocation| {
            relocation.instruction_index == start + 1
                && relocation.kind == RelocationKind::Addr16Lo
        })?;
        if external_relocation_target(high) != external_relocation_target(low) {
            return None;
        }

        Some(AlternatingByteStorePacket {
            start,
            object: *object,
            first_offset: *first_offset,
            first_constant: *first_constant,
            second_constant: *second_constant,
        })
    })
}

fn external_relocation_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation(
        instruction_index: usize,
        kind: RelocationKind,
    ) -> mwcc_machine_code::Relocation {
        mwcc_machine_code::Relocation {
            instruction_index,
            kind,
            target: mwcc_machine_code::RelocationTarget::External("byte_source".to_owned()),
        }
    }

    #[test]
    fn recognizes_a_relocated_load_followed_by_alternating_byte_constants() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(3, 0),
            Instruction::LoadWord { d: 0, a: 3, offset: 0 },
            Instruction::StoreByte { s: 0, a: 31, offset: 2 },
            Instruction::load_immediate(0, 4),
            Instruction::StoreByte { s: 0, a: 31, offset: 3 },
            Instruction::load_immediate(0, 8),
            Instruction::StoreByte { s: 0, a: 31, offset: 4 },
            Instruction::load_immediate(0, 4),
            Instruction::StoreByte { s: 0, a: 31, offset: 5 },
            Instruction::load_immediate(0, 8),
            Instruction::StoreByte { s: 0, a: 31, offset: 6 },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
        ];
        let relocations = vec![
            relocation(0, RelocationKind::Addr16Ha),
            relocation(1, RelocationKind::Addr16Lo),
        ];

        let packet = AlternatingByteStorePacket {
                start: 0,
                object: 31,
                first_offset: 2,
                first_constant: 4,
                second_constant: 8,
            };
        assert_eq!(
            alternating_byte_store_packet(&instructions, &relocations),
            Some(packet)
        );
        rewrite_alternating_byte_store_packet(&mut instructions, packet);
        assert_eq!(instructions.len(), 13);
        assert_eq!(
            instructions[12],
            Instruction::LoadWord { d: 31, a: 1, offset: 12 }
        );
        assert_eq!(instructions[3], Instruction::load_immediate(4, 4));
        assert_eq!(instructions[4], Instruction::load_immediate(0, 8));
        assert_eq!(
            instructions[5],
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 24,
                end: 31,
            }
        );
    }
}
