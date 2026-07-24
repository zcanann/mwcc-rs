//! Direct-cursor scheduling for the guarded copy-mode display-list packets.
//!
//! Two adjacent eight-byte packets are built from a command word plus a saved
//! value. MWCC stores through the live cursor and advances it after each packet,
//! avoiding both a packet alias and a scratch copy of the saved value.

#[allow(unused_imports)]
use super::*;

const CONDITIONAL_COPY_PACKET: [usize; 9] = [2, 3, 4, 5, 6, 8, 1, 0, 7];
const TERMINAL_COPY_PACKET: [usize; 6] = [2, 3, 5, 1, 0, 4];

impl Generator {
    pub(super) fn schedule_structured_copy_packets(&mut self) {
        narrow_copy_mode_dimensions(&mut self.output.instructions);
        let Some(first) = conditional_copy_packet(&self.output.instructions) else {
            return;
        };
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[first.start + 6]
        else {
            unreachable!("the conditional command store was matched")
        };
        *a = first.cursor;
        let Instruction::StoreWord { s, a, .. } = &mut self.output.instructions[first.start + 8]
        else {
            unreachable!("the conditional saved-value store was matched")
        };
        *s = first.saved;
        *a = first.cursor;
        self.permute_copy_packet(first.start, &CONDITIONAL_COPY_PACKET);
        self.remove_copy_packet_instruction(first.start + 8);
        self.remove_copy_packet_instruction(first.start + 7);

        let Some(second) = terminal_copy_packet(&self.output.instructions) else {
            return;
        };
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[second.start + 3]
        else {
            unreachable!("the terminal command store was matched")
        };
        *a = second.cursor;
        let Instruction::StoreWord { s, a, .. } = &mut self.output.instructions[second.start + 5]
        else {
            unreachable!("the terminal saved-value store was matched")
        };
        *s = second.saved;
        *a = second.cursor;
        self.permute_copy_packet(second.start, &TERMINAL_COPY_PACKET);
        self.remove_copy_packet_instruction(second.start + 5);
        self.remove_copy_packet_instruction(second.start + 4);
    }

    fn permute_copy_packet(&mut self, start: usize, schedule: &[usize]) {
        let mut current: Vec<usize> = (0..schedule.len()).collect();
        for (destination, &original) in schedule.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the copy packet schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }

    fn remove_copy_packet_instruction(&mut self, index: usize) {
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
}

fn narrow_copy_mode_dimensions(instructions: &mut [Instruction]) {
    for start in 0..instructions.len().saturating_sub(2) {
        let replacement = match &instructions[start..start + 3] {
            [Instruction::LoadHalfwordZero {
                d: loaded,
                a: object,
                offset: load_offset,
            }, Instruction::ShiftLeftImmediate {
                a: shifted,
                s: shift_source,
                shift: 2,
            }, Instruction::StoreHalfword {
                s: stored,
                a: result,
                offset: store_offset,
            }] if loaded == shift_source
                && shifted == stored
                && matches!((*load_offset, *store_offset), (8, 6) | (10, 14))
                && object != result =>
            {
                Some((*shifted, *shift_source))
            }
            _ => None,
        };
        if let Some((destination, source)) = replacement {
            instructions[start + 1] = Instruction::RotateAndMask {
                a: destination,
                s: source,
                shift: 2,
                begin: 16,
                end: 29,
            };
        }
    }
}

#[derive(Clone, Copy)]
struct CopyPacket {
    start: usize,
    cursor: u8,
    saved: u8,
}

fn conditional_copy_packet(instructions: &[Instruction]) -> Option<CopyPacket> {
    instructions
        .windows(9)
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
                    Instruction::LoadHalfwordZero { .. },
                    Instruction::OrImmediateShifted { immediate: 32, .. },
                    Instruction::AndContiguousMask {
                        begin: 8,
                        end: 31,
                        ..
                    },
                    Instruction::OrImmediateShifted {
                        immediate: 61184,
                        ..
                    },
                    Instruction::StoreWord {
                        a: command_base,
                        offset: 0,
                        ..
                    },
                    Instruction::Or {
                        a: temporary,
                        s: saved,
                        b: saved_b,
                    },
                    Instruction::StoreWord {
                        s: stored,
                        a: saved_base,
                        offset: 4,
                    },
                ] if cursor == cursor_b
                    && cursor == bumped
                    && cursor == bump_source
                    && packet == command_base
                    && packet == saved_base
                    && saved == saved_b
                    && temporary == stored
            )
            .then(|| CopyPacket {
                start,
                cursor: match window[0] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the packet alias was matched"),
                },
                saved: match window[7] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the saved-value copy was matched"),
                },
            })
        })
}

fn terminal_copy_packet(instructions: &[Instruction]) -> Option<CopyPacket> {
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
                        immediate: 2560,
                        ..
                    },
                    Instruction::StoreWord {
                        a: command_base,
                        offset: 0,
                        ..
                    },
                    Instruction::Or {
                        a: temporary,
                        s: saved,
                        b: saved_b,
                    },
                    Instruction::StoreWord {
                        s: stored,
                        a: saved_base,
                        offset: 4,
                    },
                ] if cursor == cursor_b
                    && cursor == bumped
                    && cursor == bump_source
                    && packet == command_base
                    && packet == saved_base
                    && saved == saved_b
                    && temporary == stored
            )
            .then(|| CopyPacket {
                start,
                cursor: match window[0] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the packet alias was matched"),
                },
                saved: match window[4] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the saved-value copy was matched"),
                },
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_terminal_copy_packet() {
        let instructions = vec![
            Instruction::move_register(57, 37),
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
            Instruction::load_immediate_shifted(0, 2560),
            Instruction::StoreWord {
                s: 0,
                a: 57,
                offset: 0,
            },
            Instruction::move_register(0, 36),
            Instruction::StoreWord {
                s: 0,
                a: 57,
                offset: 4,
            },
        ];
        let packet = terminal_copy_packet(&instructions).expect("the packet should match");
        assert_eq!((packet.start, packet.cursor, packet.saved), (0, 37, 36));
    }

    #[test]
    fn narrows_copy_mode_dimensions_after_scaling() {
        let mut instructions = vec![
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 33,
                offset: 8,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 36,
                offset: 6,
            },
        ];
        narrow_copy_mode_dimensions(&mut instructions);
        assert!(matches!(
            instructions[1],
            Instruction::RotateAndMask {
                a: 0,
                s: 0,
                shift: 2,
                begin: 16,
                end: 29,
            }
        ));
    }
}
