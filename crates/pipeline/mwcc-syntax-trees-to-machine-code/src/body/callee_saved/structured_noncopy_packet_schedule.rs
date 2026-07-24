//! Direct-cursor scheduling for the guarded non-copy render-mode packet.
//!
//! The command and saved render mode form one eight-byte packet. MWCC computes
//! both words, stores through the live cursor, and advances it afterward,
//! avoiding a temporary packet alias.

#[allow(unused_imports)]
use super::*;

const NONCOPY_PACKET_SCHEDULE: [usize; 10] = [2, 7, 8, 3, 4, 5, 6, 9, 1, 0];

impl Generator {
    pub(super) fn schedule_structured_noncopy_packet(&mut self) {
        let Some(packet) = noncopy_packet(&self.output.instructions) else {
            return;
        };
        let start = packet.start;
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[start + 6] else {
            unreachable!("the command store was matched")
        };
        *a = packet.cursor;
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[start + 9] else {
            unreachable!("the saved-mode store was matched")
        };
        *a = packet.cursor;

        let mut current: Vec<usize> = (0..NONCOPY_PACKET_SCHEDULE.len()).collect();
        for (destination, &original) in NONCOPY_PACKET_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the non-copy packet schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        self.remove_noncopy_packet_instruction(start + 9);
    }

    pub(crate) fn finalize_structured_noncopy_packet_registers(&mut self) {
        let Some(start) = final_noncopy_packet(&self.output.instructions) else {
            return;
        };
        let [load, saved_high, saved_low, command_low, narrow, command_high, command_store, saved_store, _bump] =
            &mut self.output.instructions[start..start + 9]
        else {
            unreachable!("the final non-copy packet was matched")
        };
        let Instruction::LoadHalfwordZero { d, .. } = load else {
            unreachable!()
        };
        *d = 3;
        let Instruction::OrImmediateShifted { a, s, .. } = saved_high else {
            unreachable!()
        };
        *a = 0;
        *s = 30;
        let Instruction::OrImmediate { a, s, .. } = saved_low else {
            unreachable!()
        };
        *a = 0;
        *s = 0;
        for instruction in [command_low, narrow, command_high] {
            match instruction {
                Instruction::OrImmediate { a, s, .. }
                | Instruction::OrImmediateShifted { a, s, .. }
                | Instruction::AndContiguousMask { a, s, .. } => {
                    *a = 3;
                    *s = 3;
                }
                _ => unreachable!(),
            }
        }
        let Instruction::StoreWord { s, .. } = command_store else {
            unreachable!()
        };
        *s = 3;
        let Instruction::StoreWord { s, .. } = saved_store else {
            unreachable!()
        };
        *s = 0;
    }

    fn remove_noncopy_packet_instruction(&mut self, index: usize) {
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

fn final_noncopy_packet(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(9).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadHalfwordZero { offset: 14, .. },
                Instruction::OrImmediateShifted { immediate: 160, .. },
                Instruction::OrImmediate {
                    immediate: 12296,
                    ..
                },
                Instruction::OrImmediate {
                    immediate: 3312,
                    ..
                },
                Instruction::AndContiguousMask {
                    begin: 8,
                    end: 31,
                    ..
                },
                Instruction::OrImmediateShifted {
                    immediate: 61184,
                    ..
                },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::StoreWord { offset: 4, .. },
                Instruction::AddImmediate { immediate: 8, .. },
            ]
        )
    })
}

#[derive(Clone, Copy)]
struct NoncopyPacket {
    start: usize,
    cursor: u8,
}

fn noncopy_packet(instructions: &[Instruction]) -> Option<NoncopyPacket> {
    instructions
        .windows(10)
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
                    Instruction::LoadHalfwordZero { offset: 14, .. },
                    Instruction::OrImmediate { immediate: 3312, .. },
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
                    Instruction::OrImmediateShifted {
                        immediate: 160,
                        ..
                    },
                    Instruction::OrImmediate {
                        immediate: 12296,
                        ..
                    },
                    Instruction::StoreWord {
                        a: saved_base,
                        offset: 4,
                        ..
                    },
                ] if cursor == cursor_b
                    && cursor == bumped
                    && cursor == bump_source
                    && packet == command_base
                    && packet == saved_base
            )
            .then(|| NoncopyPacket {
                start,
                cursor: match window[0] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the packet alias was matched"),
                },
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_guarded_noncopy_packet() {
        let instructions = vec![
            Instruction::move_register(57, 37),
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 33,
                offset: 14,
            },
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: 3312,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 8,
                end: 31,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 61184,
            },
            Instruction::StoreWord {
                s: 0,
                a: 57,
                offset: 0,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 35,
                immediate: 160,
            },
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: 12296,
            },
            Instruction::StoreWord {
                s: 0,
                a: 57,
                offset: 4,
            },
        ];
        let packet = noncopy_packet(&instructions).expect("the packet should match");
        assert_eq!((packet.start, packet.cursor), (0, 37));
    }
}
