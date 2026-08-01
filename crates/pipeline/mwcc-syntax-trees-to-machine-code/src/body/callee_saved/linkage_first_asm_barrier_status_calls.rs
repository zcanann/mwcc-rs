//! Guarded status-call scheduling after a translation-unit assembly barrier.
//!
//! Build 163 completes the saved buffer copy before materializing a cached
//! global base at entry. In the final guarded call it performs the relocated
//! member load before forwarding that saved buffer. These two fixed-size
//! packets share one semantic owner so relocation movement cannot drift away
//! from the corresponding instruction schedule.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_asm_barrier_status_calls(&mut self) -> bool {
        if !self.preceded_by_asm
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_callee_saved_frame_layout
                != LegacyCalleeSavedFrameLayout::PreserveLogicalSize
        {
            return false;
        }

        let mut scheduled = false;
        if let Some(packet) = derived_status_entry_packet(
            &self.output.instructions,
            &self.output.relocations,
        ) {
            rewrite_derived_status_entry_packet(
                &mut self.output.instructions,
                &mut self.output.relocations,
                packet,
            );
            scheduled = true;
        } else if let Some(packet) = status_entry_packet(
            &self.output.instructions,
            &self.output.relocations,
        ) {
            rewrite_status_entry_packet(
                &mut self.output.instructions,
                &mut self.output.relocations,
                packet,
            );
            scheduled = true;
        }
        if let Some(packet) = narrowed_word_status_tail_packet(
            &self.output.instructions,
            &self.output.relocations,
        ) {
            rewrite_narrowed_word_status_tail_packet(
                &mut self.output.instructions,
                &mut self.output.relocations,
                packet,
            );
            scheduled = true;
        } else if let Some(packet) = status_tail_packet(
            &self.output.instructions,
            &self.output.relocations,
        ) {
            rewrite_status_tail_packet(
                &mut self.output.instructions,
                &mut self.output.relocations,
                packet,
            );
            scheduled = true;
        }
        scheduled
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DerivedStatusEntryPacket {
    start: usize,
    global_home: u8,
    buffer_home: u8,
    global_offset: i16,
}

fn derived_status_entry_packet(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<DerivedStatusEntryPacket> {
    instructions.windows(7).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediateShifted { d: address, a: 0, .. },
            Instruction::AddImmediate { d: low, a: low_base, immediate: 0 },
            Instruction::AddImmediate { d: global_home, a: member_base, immediate: global_offset },
            Instruction::AddImmediate { d: buffer_home, a: 3, immediate: 0 },
            Instruction::Or { a: 3, s: forwarded, b: repeated },
            Instruction::LoadWord { d: 4, a: load_base, .. },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        (*address == *low
            && low == low_base
            && low == member_base
            && global_home == load_base
            && buffer_home == forwarded
            && forwarded == repeated
            && *global_offset != 0
            && relocated_external_pair(relocations, start, start + 1))
            .then_some(DerivedStatusEntryPacket {
                start,
                global_home: *global_home,
                buffer_home: *buffer_home,
                global_offset: *global_offset,
            })
    })
}

fn rewrite_derived_status_entry_packet(
    instructions: &mut [Instruction],
    relocations: &mut [mwcc_machine_code::Relocation],
    packet: DerivedStatusEntryPacket,
) {
    let start = packet.start;
    instructions[start] = Instruction::move_register(packet.buffer_home, 3);
    instructions[start + 1] = Instruction::load_immediate_shifted(3, 0);
    instructions[start + 2] = Instruction::AddImmediate {
        d: 3,
        a: 3,
        immediate: 0,
    };
    instructions[start + 3] = Instruction::AddImmediate {
        d: packet.global_home,
        a: 3,
        immediate: packet.global_offset,
    };
    instructions[start + 4] = Instruction::LoadWord {
        d: 4,
        a: packet.global_home,
        offset: 0,
    };
    instructions[start + 5] = Instruction::move_register(3, packet.buffer_home);
    move_relocation(relocations, start, start + 1, RelocationKind::Addr16Ha);
    move_relocation(relocations, start + 1, start + 2, RelocationKind::Addr16Lo);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusEntryPacket {
    start: usize,
    global_home: u8,
    buffer_home: u8,
}

fn status_entry_packet(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<StatusEntryPacket> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediateShifted { d: address, a: 0, .. },
            Instruction::AddImmediate { d: global_home, a: low_base, immediate: 0 },
            Instruction::AddImmediate { d: buffer_home, a: 3, immediate: 0 },
            Instruction::Or { a: 3, s: forwarded, b: repeated },
            Instruction::LoadWord { d: 4, a: load_base, .. },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        (*address == *low_base
            && global_home == load_base
            && buffer_home == forwarded
            && forwarded == repeated
            && global_home != buffer_home
            && relocated_external_pair(relocations, start, start + 1))
            .then_some(StatusEntryPacket {
                start,
                global_home: *global_home,
                buffer_home: *buffer_home,
            })
    })
}

fn rewrite_status_entry_packet(
    instructions: &mut [Instruction],
    relocations: &mut [mwcc_machine_code::Relocation],
    packet: StatusEntryPacket,
) {
    let start = packet.start;
    instructions[start] = Instruction::move_register(packet.buffer_home, 3);
    instructions[start + 1] = Instruction::load_immediate_shifted(3, 0);
    instructions[start + 2] = Instruction::AddImmediate {
        d: packet.global_home,
        a: 3,
        immediate: 0,
    };
    instructions[start + 3] = Instruction::LoadWord {
        d: 4,
        a: packet.global_home,
        offset: 0,
    };
    instructions[start + 4] = Instruction::move_register(3, packet.buffer_home);
    move_relocation(relocations, start, start + 1, RelocationKind::Addr16Ha);
    move_relocation(relocations, start + 1, start + 2, RelocationKind::Addr16Lo);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusTailPacket {
    start: usize,
    buffer_home: u8,
    member_offset: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NarrowedWordStatusTailPacket {
    start: usize,
    buffer_home: u8,
    member_offset: i16,
}

fn narrowed_word_status_tail_packet(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<NarrowedWordStatusTailPacket> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            forward,
            Instruction::AddImmediateShifted { d: address, a: 0, .. },
            Instruction::AddImmediate { d: member_base, a: low_base, immediate: 0 },
            Instruction::LoadWord { d: 0, a: load_base, offset },
            narrow,
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        let buffer_home = saved_buffer_copy(forward)?;
        (address == low_base
            && address == member_base
            && member_base == load_base
            && unsigned_halfword_clear(narrow)
            && relocated_external_pair(relocations, start + 1, start + 2))
            .then_some(NarrowedWordStatusTailPacket {
                start,
                buffer_home,
                member_offset: *offset,
            })
    })
}

fn unsigned_halfword_clear(instruction: &Instruction) -> bool {
    matches!(instruction,
        Instruction::ClearLeftImmediate { a: 4, s: 0, clear: 16 }
            | Instruction::RotateAndMask {
                a: 4,
                s: 0,
                shift: 0,
                begin: 16,
                end: 31,
            })
}

fn rewrite_narrowed_word_status_tail_packet(
    instructions: &mut [Instruction],
    relocations: &mut [mwcc_machine_code::Relocation],
    packet: NarrowedWordStatusTailPacket,
) {
    let start = packet.start;
    instructions[start] = Instruction::load_immediate_shifted(3, 0);
    instructions[start + 1] = Instruction::AddImmediate {
        d: 3,
        a: 3,
        immediate: 0,
    };
    instructions[start + 2] = Instruction::LoadWord {
        d: 0,
        a: 3,
        offset: packet.member_offset,
    };
    instructions[start + 3] = Instruction::move_register(3, packet.buffer_home);
    move_relocation(relocations, start + 1, start, RelocationKind::Addr16Ha);
    move_relocation(relocations, start + 2, start + 1, RelocationKind::Addr16Lo);
}

fn status_tail_packet(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<StatusTailPacket> {
    instructions.windows(5).enumerate().find_map(|(start, window)| {
        let [
            forward,
            Instruction::AddImmediateShifted { d: address, a: 0, .. },
            Instruction::AddImmediate { d: member_base, a: low_base, immediate: 0 },
            Instruction::LoadHalfwordZero { d: 4, a: load_base, offset },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return None;
        };
        let buffer_home = saved_buffer_copy(forward)?;
        (address == low_base
            && address == member_base
            && member_base == load_base
            && relocated_external_pair(relocations, start + 1, start + 2))
            .then_some(StatusTailPacket {
                start,
                buffer_home,
                member_offset: *offset,
            })
    })
}

fn saved_buffer_copy(instruction: &Instruction) -> Option<u8> {
    match *instruction {
        Instruction::Or { a: 3, s, b } if s == b && s >= 14 => Some(s),
        Instruction::AddImmediate {
            d: 3,
            a,
            immediate: 0,
        } if a >= 14 => Some(a),
        _ => None,
    }
}

fn rewrite_status_tail_packet(
    instructions: &mut [Instruction],
    relocations: &mut [mwcc_machine_code::Relocation],
    packet: StatusTailPacket,
) {
    let start = packet.start;
    instructions[start] = Instruction::load_immediate_shifted(3, 0);
    instructions[start + 1] = Instruction::AddImmediate {
        d: 3,
        a: 3,
        immediate: 0,
    };
    instructions[start + 2] = Instruction::LoadHalfwordZero {
        d: 4,
        a: 3,
        offset: packet.member_offset,
    };
    instructions[start + 3] = Instruction::move_register(3, packet.buffer_home);
    move_relocation(relocations, start + 1, start, RelocationKind::Addr16Ha);
    move_relocation(relocations, start + 2, start + 1, RelocationKind::Addr16Lo);
}

fn relocated_external_pair(
    relocations: &[mwcc_machine_code::Relocation],
    high_index: usize,
    low_index: usize,
) -> bool {
    let high = relocations.iter().find(|relocation| {
        relocation.instruction_index == high_index
            && relocation.kind == RelocationKind::Addr16Ha
    });
    let low = relocations.iter().find(|relocation| {
        relocation.instruction_index == low_index
            && relocation.kind == RelocationKind::Addr16Lo
    });
    matches!((high, low), (Some(high), Some(low))
        if external_target(high).is_some()
            && external_target(high) == external_target(low))
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

fn move_relocation(
    relocations: &mut [mwcc_machine_code::Relocation],
    from: usize,
    to: usize,
    kind: RelocationKind,
) {
    let relocation = relocations
        .iter_mut()
        .find(|relocation| {
            relocation.instruction_index == from && relocation.kind == kind
        })
        .expect("the recognized status packet retains its relocation");
    relocation.instruction_index = to;
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
            target: mwcc_machine_code::RelocationTarget::External("status".into()),
        }
    }

    #[test]
    fn rewrites_entry_and_tail_without_changing_packet_sizes() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate { d: 31, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::move_register(3, 30),
            Instruction::LoadWord { d: 4, a: 31, offset: 0 },
            Instruction::BranchAndLink { target: "append".into() },
            Instruction::move_register(3, 30),
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::LoadHalfwordZero { d: 4, a: 4, offset: 8 },
            Instruction::BranchAndLink { target: "append_short".into() },
        ];
        let mut relocations = vec![
            relocation(0, RelocationKind::Addr16Ha),
            relocation(1, RelocationKind::Addr16Lo),
            relocation(7, RelocationKind::Addr16Ha),
            relocation(8, RelocationKind::Addr16Lo),
        ];

        let entry = status_entry_packet(&instructions, &relocations)
            .expect("the status entry should match");
        rewrite_status_entry_packet(&mut instructions, &mut relocations, entry);
        let tail = status_tail_packet(&instructions, &relocations)
            .expect("the status tail should match");
        rewrite_status_tail_packet(&mut instructions, &mut relocations, tail);

        assert_eq!(instructions.len(), 11);
        assert_eq!(instructions[0], Instruction::move_register(30, 3));
        assert_eq!(instructions[3], Instruction::LoadWord { d: 4, a: 31, offset: 0 });
        assert_eq!(instructions[6], Instruction::load_immediate_shifted(3, 0));
        assert_eq!(instructions[9], Instruction::move_register(3, 30));
        assert_eq!(
            relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 6, 7]
        );
    }

    #[test]
    fn rewrites_derived_entry_and_narrowed_word_tail() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 4, immediate: 128 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::move_register(3, 30),
            Instruction::LoadWord { d: 4, a: 31, offset: 0 },
            Instruction::BranchAndLink { target: "append".into() },
            Instruction::move_register(3, 30),
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::LoadWord { d: 0, a: 4, offset: 760 },
            Instruction::RotateAndMask { a: 4, s: 0, shift: 0, begin: 16, end: 31 },
            Instruction::BranchAndLink { target: "append_short".into() },
        ];
        let mut relocations = vec![
            relocation(0, RelocationKind::Addr16Ha),
            relocation(1, RelocationKind::Addr16Lo),
            relocation(8, RelocationKind::Addr16Ha),
            relocation(9, RelocationKind::Addr16Lo),
        ];

        let entry = derived_status_entry_packet(&instructions, &relocations)
            .expect("the derived status entry should match");
        rewrite_derived_status_entry_packet(&mut instructions, &mut relocations, entry);
        let tail = narrowed_word_status_tail_packet(&instructions, &relocations)
            .expect("the narrowed status tail should match");
        rewrite_narrowed_word_status_tail_packet(
            &mut instructions,
            &mut relocations,
            tail,
        );

        assert_eq!(instructions.len(), 13);
        assert_eq!(instructions[0], Instruction::move_register(30, 3));
        assert_eq!(
            instructions[3],
            Instruction::AddImmediate { d: 31, a: 3, immediate: 128 }
        );
        assert_eq!(instructions[7], Instruction::load_immediate_shifted(3, 0));
        assert_eq!(instructions[10], Instruction::move_register(3, 30));
        assert_eq!(
            relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 7, 8]
        );
    }
}
