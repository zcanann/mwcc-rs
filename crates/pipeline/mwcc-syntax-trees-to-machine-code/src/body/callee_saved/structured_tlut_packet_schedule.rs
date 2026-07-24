//! Instruction-count normalization for a guarded TLUT display-list packet.
//!
//! The 48-byte packet contains three zero words and one packed count field.
//! MWCC retains one zero register across the packet and combines the unsigned
//! narrowing plus left shift into one rotate-mask instruction.

#[allow(unused_imports)]
use super::*;

const TLUT_PACKET_SCHEDULE: [usize; 28] = [
    0, 8, 1, 9, 4, 6, 2, 11, 13, 16, 3, 23, 5, 7, 10, 12, 14, 15, 17, 18, 19, 20, 21, 22, 24, 25,
    26, 27,
];

impl Generator {
    pub(super) fn schedule_structured_tlut_packet(&mut self) {
        if let Some(zeros) = tlut_zero_stores(&self.output.instructions) {
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[zeros.first_load]
            else {
                unreachable!("the first TLUT zero was matched")
            };
            *d = 7;
            for store in [zeros.first_store, zeros.second_store, zeros.third_store] {
                let Instruction::StoreWord { s, .. } = &mut self.output.instructions[store] else {
                    unreachable!("the TLUT zero store was matched")
                };
                *s = 7;
            }
            self.remove_tlut_instruction(zeros.third_load);
            self.remove_tlut_instruction(zeros.second_load);
        }

        let Some(mask) = tlut_count_mask(&self.output.instructions) else {
            return;
        };
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[mask.subtract]
        else {
            unreachable!("the count decrement was matched")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT;
        self.output.instructions[mask.clear] = Instruction::RotateAndMask {
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            shift: 14,
            begin: 8,
            end: 17,
        };
        let Instruction::OrImmediateShifted { a, s, .. } =
            &mut self.output.instructions[mask.combine]
        else {
            unreachable!("the count command merge was matched")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT;
        *s = Eabi::FIRST_GENERAL_ARGUMENT;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[mask.store] else {
            unreachable!("the packed count store was matched")
        };
        *s = Eabi::FIRST_GENERAL_ARGUMENT;
        self.remove_tlut_instruction(mask.shift);
        self.schedule_tlut_packet_order();
        self.schedule_tlut_alternate_packet();
    }

    fn remove_tlut_instruction(&mut self, index: usize) {
        let old_len = self.output.instructions.len();
        self.output.instructions.remove(index);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < index {
                    old
                } else if old == index {
                    index.saturating_sub(1)
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }

    fn schedule_tlut_packet_order(&mut self) {
        let Some(region) = tlut_packet_region(&self.output.instructions) else {
            return;
        };
        let mut current: Vec<usize> = (0..TLUT_PACKET_SCHEDULE.len()).collect();
        for (destination, &original) in TLUT_PACKET_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the TLUT schedule is a permutation");
            if source != destination {
                self.move_instruction_before(region.start + source, region.start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        assign_tlut_packet_registers(
            &mut self.output.instructions[region.start..region.start + 28],
            region.cursor,
            region.object,
        );
    }

    fn schedule_tlut_alternate_packet(&mut self) {
        let Some(region) = tlut_alternate_packet(&self.output.instructions) else {
            return;
        };
        let start = region.start;
        self.move_instruction_before(start + 2, start);
        self.move_instruction_before(start + 4, start + 1);
        self.move_instruction_before(start + 4, start + 2);
        self.move_instruction_before(start + 5, start + 3);

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[start]
        else {
            unreachable!("the alternate command was scheduled first")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT;
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!("the alternate zero was scheduled second")
        };
        *d = 0;
        let Instruction::StoreWord { s, a, .. } = &mut self.output.instructions[start + 2] else {
            unreachable!("the alternate command store was scheduled third")
        };
        *s = Eabi::FIRST_GENERAL_ARGUMENT;
        *a = region.cursor;
        let Instruction::StoreWord { s, a, .. } = &mut self.output.instructions[start + 3] else {
            unreachable!("the alternate zero store was scheduled fourth")
        };
        *s = 0;
        *a = region.cursor;
        self.remove_tlut_instruction(start + 4);
    }
}

#[derive(Clone, Copy)]
struct TlutPacketRegion {
    start: usize,
    cursor: u8,
    object: u8,
}

fn tlut_packet_region(instructions: &[Instruction]) -> Option<TlutPacketRegion> {
    instructions
        .windows(28)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::AddImmediateShifted {
                        a: 0,
                        immediate: -752,
                        ..
                    },
                    Instruction::StoreWord {
                        a: cursor,
                        offset: 0,
                        ..
                    },
                    Instruction::LoadWord {
                        a: object,
                        offset: 4,
                        ..
                    },
                    Instruction::StoreWord {
                        a: second_store_base,
                        offset: 4,
                        ..
                    },
                    ..,
                    Instruction::Branch { .. },
                ] if cursor == second_store_base && cursor != object
            )
            .then(|| TlutPacketRegion {
                start,
                cursor: match window[1] {
                    Instruction::StoreWord { a, .. } => a,
                    _ => unreachable!("the first packet store was matched"),
                },
                object: match window[2] {
                    Instruction::LoadWord { a, .. } => a,
                    _ => unreachable!("the packet source load was matched"),
                },
            })
        })
}

fn assign_tlut_packet_registers(instructions: &mut [Instruction], cursor: u8, object: u8) {
    instructions[0] = Instruction::load_immediate_shifted(0, -752);
    instructions[1] = Instruction::load_immediate_shifted(3, -2816);
    instructions[2] = Instruction::StoreWord {
        s: 0,
        a: cursor,
        offset: 0,
    };
    instructions[3] = Instruction::AddImmediate {
        d: 6,
        a: 3,
        immediate: 256,
    };
    instructions[4] = Instruction::load_immediate_shifted(8, -6144);
    instructions[5] = Instruction::load_immediate(7, 0);
    instructions[6] = Instruction::LoadWord {
        d: 0,
        a: object,
        offset: 4,
    };
    instructions[7] = Instruction::load_immediate_shifted(5, 1792);
    instructions[8] = Instruction::load_immediate_shifted(4, -6656);
    instructions[9] = Instruction::load_immediate_shifted(3, -4096);
    for (index, source, offset) in [
        (10, 0, 4),
        (12, 8, 8),
        (13, 7, 12),
        (14, 6, 16),
        (15, 5, 20),
        (16, 4, 24),
        (17, 7, 28),
        (18, 3, 32),
        (23, 3, 36),
        (24, 0, 40),
        (25, 7, 44),
    ] {
        instructions[index] = Instruction::StoreWord {
            s: source,
            a: cursor,
            offset,
        };
    }
    instructions[11] = Instruction::load_immediate_shifted(0, -6400);
    instructions[19] = Instruction::LoadHalfwordZero {
        d: 3,
        a: object,
        offset: 16,
    };
    instructions[20] = Instruction::AddImmediate {
        d: 3,
        a: 3,
        immediate: -1,
    };
    instructions[21] = Instruction::RotateAndMask {
        a: 3,
        s: 3,
        shift: 14,
        begin: 8,
        end: 17,
    };
    instructions[22] = Instruction::OrImmediateShifted {
        a: 3,
        s: 3,
        immediate: 1792,
    };
}

#[derive(Clone, Copy)]
struct TlutAlternatePacket {
    start: usize,
    cursor: u8,
}

fn tlut_alternate_packet(instructions: &[Instruction]) -> Option<TlutAlternatePacket> {
    instructions
        .windows(6)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::Or {
                        a: packet,
                        s: cursor,
                        b: cursor_b,
                    },
                    Instruction::AddImmediate {
                        d: bumped,
                        a: bump_source,
                        immediate: 8,
                    },
                    Instruction::AddImmediateShifted {
                        d: command,
                        a: 0,
                        immediate: -6400,
                    },
                    Instruction::StoreWord {
                        s: stored_command,
                        a: command_base,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: zero,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::StoreWord {
                        s: stored_zero,
                        a: zero_base,
                        offset: 4,
                    },
                ] if cursor == cursor_b
                    && cursor == bumped
                    && cursor == bump_source
                    && packet == command_base
                    && packet == zero_base
                    && command == stored_command
                    && zero == stored_zero
            )
            .then(|| TlutAlternatePacket {
                start,
                cursor: match window[0] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the alternate packet alias was matched"),
                },
            })
        })
}

#[derive(Clone, Copy)]
struct TlutZeroStores {
    first_load: usize,
    first_store: usize,
    second_load: usize,
    second_store: usize,
    third_load: usize,
    third_store: usize,
}

fn tlut_zero_stores(instructions: &[Instruction]) -> Option<TlutZeroStores> {
    let (first_load, base) = instructions
        .windows(2)
        .enumerate()
        .find_map(|(index, window)| zero_word_store(window, 12).map(|base| (index, base)))?;
    let second_load = (first_load + 2..instructions.len().saturating_sub(1))
        .find(|&index| zero_word_store(&instructions[index..index + 2], 28) == Some(base))?;
    let third_load = (second_load + 2..instructions.len().saturating_sub(1))
        .find(|&index| zero_word_store(&instructions[index..index + 2], 44) == Some(base))?;
    if instructions[first_load + 2..third_load]
        .iter()
        .any(is_tlut_barrier)
    {
        return None;
    }
    Some(TlutZeroStores {
        first_load,
        first_store: first_load + 1,
        second_load,
        second_store: second_load + 1,
        third_load,
        third_store: third_load + 1,
    })
}

fn zero_word_store(window: &[Instruction], offset: i16) -> Option<u8> {
    match window {
        [Instruction::AddImmediate {
            d: zero,
            a: 0,
            immediate: 0,
        }, Instruction::StoreWord {
            s,
            a,
            offset: store_offset,
        }, ..]
            if zero == s && *store_offset == offset =>
        {
            Some(*a)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct TlutCountMask {
    subtract: usize,
    clear: usize,
    shift: usize,
    combine: usize,
    store: usize,
}

fn tlut_count_mask(instructions: &[Instruction]) -> Option<TlutCountMask> {
    instructions
        .windows(5)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: decremented,
                        immediate: -1,
                        ..
                    },
                Instruction::AndContiguousMask {
                    a: narrowed,
                    s: decrement_source,
                    begin: 22,
                    end: 31,
                    },
                    Instruction::ShiftLeftImmediate {
                        a: shifted,
                        s: narrow_source,
                        shift: 14,
                    },
                    Instruction::OrImmediateShifted {
                        a: combined,
                        s: shift_source,
                        immediate: 1792,
                    },
                    Instruction::StoreWord {
                        s: stored,
                        offset: 36,
                        ..
                    },
                ] if decremented == decrement_source
                    && narrowed == narrow_source
                    && shifted == shift_source
                    && combined == stored
            )
            .then_some(TlutCountMask {
                subtract: start,
                clear: start + 1,
                shift: start + 2,
                combine: start + 3,
                store: start + 4,
            })
        })
}

fn is_tlut_barrier(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::BranchConditionalForward { .. }
            | Instruction::Branch { .. }
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchToLinkRegister
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_three_zero_words_in_one_tlut_packet() {
        let instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 12,
            },
            Instruction::load_immediate_shifted(0, -2816),
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 28,
            },
            Instruction::load_immediate_shifted(0, -4096),
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 44,
            },
        ];
        let zeros = tlut_zero_stores(&instructions).expect("the zero stores should match");
        assert_eq!(
            (
                zeros.first_load,
                zeros.second_load,
                zeros.third_load,
                zeros.third_store,
            ),
            (0, 3, 6, 7)
        );
    }

    #[test]
    fn recognizes_the_narrowed_shifted_tlut_count() {
        let instructions = vec![
            Instruction::AddImmediate {
                d: 0,
                a: 47,
                immediate: -1,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 22,
                end: 31,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 14,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 1792,
            },
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 36,
            },
        ];
        let mask = tlut_count_mask(&instructions).expect("the count mask should match");
        assert_eq!(
            (
                mask.subtract,
                mask.clear,
                mask.shift,
                mask.combine,
                mask.store
            ),
            (0, 1, 2, 3, 4)
        );
    }

    #[test]
    fn recognizes_the_normalized_tlut_packet_region() {
        let mut instructions = vec![Instruction::load_immediate(0, 0); 28];
        instructions[0] = Instruction::load_immediate_shifted(0, -752);
        instructions[1] = Instruction::StoreWord {
            s: 0,
            a: 37,
            offset: 0,
        };
        instructions[2] = Instruction::LoadWord {
            d: 0,
            a: 33,
            offset: 4,
        };
        instructions[3] = Instruction::StoreWord {
            s: 0,
            a: 37,
            offset: 4,
        };
        instructions[27] = Instruction::Branch { target: 32 };
        let region = tlut_packet_region(&instructions).expect("the packet region should match");
        assert_eq!((region.start, region.cursor, region.object), (0, 37, 33));
    }

    #[test]
    fn recognizes_the_alternate_single_command_packet() {
        let instructions = vec![
            Instruction::move_register(38, 37),
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
            Instruction::load_immediate_shifted(0, -6400),
            Instruction::StoreWord {
                s: 0,
                a: 38,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 38,
                offset: 4,
            },
        ];
        let region =
            tlut_alternate_packet(&instructions).expect("the alternate packet should match");
        assert_eq!((region.start, region.cursor), (0, 37));
    }
}
