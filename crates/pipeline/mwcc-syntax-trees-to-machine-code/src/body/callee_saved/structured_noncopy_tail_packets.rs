//! Direct-cursor scheduling for the non-copy arm's three terminal packets.
//!
//! Each packet is emitted through the live cursor and advances only after its
//! stores. Virtual scheduling removes cursor aliases and saved-value copies;
//! final physical normalization assigns MWCC's measured scratch roles.

#[allow(unused_imports)]
use super::*;

const MODE_PACKET: [usize; 8] = [2, 5, 3, 4, 6, 7, 1, 0];
const OBJECT_PACKET: [usize; 9] = [0, 2, 1, 4, 3, 5, 7, 8, 6];
const PIPE_PACKET: [usize; 7] = [2, 6, 3, 4, 5, 1, 0];

impl Generator {
    pub(super) fn schedule_structured_noncopy_tail_packets(&mut self) {
        if let Some(packet) = guarded_mode_packet(&self.output.instructions) {
            for store in [packet.start + 4, packet.start + 7] {
                let Instruction::StoreWord { a, .. } = &mut self.output.instructions[store] else {
                    unreachable!("the guarded mode store was matched")
                };
                *a = packet.cursor;
            }
            self.permute_noncopy_tail(packet.start, &MODE_PACKET);
            self.remove_noncopy_tail_instruction(packet.start + 7);
        }
        if let Some(packet) = terminal_object_packet(&self.output.instructions) {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[packet.start + 7]
            else {
                unreachable!("the terminal saved-object store was matched")
            };
            *s = packet.saved;
            self.permute_noncopy_tail(packet.start, &OBJECT_PACKET);
            self.remove_noncopy_tail_instruction(packet.start + 8);
        }
        if let Some(packet) = final_pipe_packet(&self.output.instructions) {
            for store in [packet.start + 3, packet.start + 5] {
                let Instruction::StoreWord { a, .. } = &mut self.output.instructions[store] else {
                    unreachable!("the final pipe store was matched")
                };
                *a = packet.cursor;
            }
            self.permute_noncopy_tail(packet.start, &PIPE_PACKET);
            self.remove_noncopy_tail_instruction(packet.start + 6);
        }
    }

    pub(crate) fn finalize_structured_noncopy_tail_packet_registers(&mut self) {
        if let Some(start) = final_guarded_mode_packet(&self.output.instructions) {
            let [first_high, second_high, first_low, first_store, second_low, second_store, _bump] =
                &mut self.output.instructions[start..start + 7]
            else {
                unreachable!()
            };
            set_shifted_destination(first_high, 4);
            set_shifted_destination(second_high, 3);
            set_add_low(first_low, 0, 4);
            set_store_source(first_store, 0);
            set_add_low(second_low, 0, 3);
            set_store_source(second_store, 0);
        }
        if let Some(start) = final_terminal_object_packet(&self.output.instructions) {
            let [first_high, small, first_store, second_high, small_store, second_store, saved_store, _bump] =
                &mut self.output.instructions[start..start + 8]
            else {
                unreachable!()
            };
            set_shifted_destination(first_high, 0);
            let Instruction::AddImmediate { d, .. } = small else {
                unreachable!()
            };
            *d = 3;
            set_store_source(first_store, 0);
            set_shifted_destination(second_high, 0);
            set_store_source(small_store, 3);
            set_store_source(second_store, 0);
            set_store_source(saved_store, 31);
        }
    }

    fn permute_noncopy_tail(&mut self, start: usize, schedule: &[usize]) {
        let mut current: Vec<usize> = (0..schedule.len()).collect();
        for (destination, &original) in schedule.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the tail packet schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }

    fn remove_noncopy_tail_instruction(&mut self, index: usize) {
        crate::remove_instruction_retargeting_to_next(self, index);
    }
}

#[derive(Clone, Copy)]
struct CursorPacket {
    start: usize,
    cursor: u8,
}

fn guarded_mode_packet(instructions: &[Instruction]) -> Option<CursorPacket> {
    instructions.windows(8).enumerate().find_map(|(start, w)| {
        matches!(w, [
            Instruction::Or { a: packet, s: cursor, b },
            Instruction::AddImmediate { d, a, immediate: 8 },
            Instruction::AddImmediateShifted { immediate: -768, .. },
            Instruction::AddImmediate { immediate: -1, .. },
            Instruction::StoreWord { a: first_base, offset: 0, .. },
            Instruction::AddImmediateShifted { immediate: -3, .. },
            Instruction::AddImmediate { immediate: -898, .. },
            Instruction::StoreWord { a: second_base, offset: 4, .. },
        ] if cursor == b && cursor == d && cursor == a && packet == first_base && packet == second_base)
        .then(|| CursorPacket { start, cursor: match w[0] { Instruction::Or { s, .. } => s, _ => unreachable!() } })
    })
}

#[derive(Clone, Copy)]
struct ObjectPacket {
    start: usize,
    saved: u8,
}

fn terminal_object_packet(instructions: &[Instruction]) -> Option<ObjectPacket> {
    instructions.windows(9).enumerate().find_map(|(start, w)| {
        matches!(w, [
            Instruction::AddImmediateShifted { immediate: 2816, .. },
            Instruction::StoreWord { offset: 0, .. },
            Instruction::AddImmediate { a: 0, immediate: 12, .. },
            Instruction::StoreWord { offset: 4, .. },
            Instruction::AddImmediateShifted { immediate: 2304, .. },
            Instruction::StoreWord { offset: 8, .. },
            Instruction::Or { a: temporary, s: saved, b },
            Instruction::StoreWord { s: stored, offset: 12, .. },
            Instruction::AddImmediate { immediate: 16, .. },
        ] if saved == b && temporary == stored)
        .then(|| ObjectPacket {
            start,
            saved: match w[6] {
                Instruction::Or { s, .. } => s,
                _ => unreachable!(),
            },
        })
    })
}

fn final_pipe_packet(instructions: &[Instruction]) -> Option<CursorPacket> {
    instructions.windows(7).enumerate().find_map(|(start, w)| {
        matches!(w, [
            Instruction::Or { a: packet, s: cursor, b },
            Instruction::AddImmediate { d, a, immediate: 8 },
            Instruction::AddImmediateShifted { immediate: -6400, .. },
            Instruction::StoreWord { a: first_base, offset: 0, .. },
            Instruction::AddImmediate { a: 0, immediate: 0, .. },
            Instruction::StoreWord { a: second_base, offset: 4, .. },
            Instruction::CompareWordImmediate { immediate: 0, .. },
        ] if cursor == b && cursor == d && cursor == a && packet == first_base && packet == second_base)
        .then(|| CursorPacket { start, cursor: match w[0] { Instruction::Or { s, .. } => s, _ => unreachable!() } })
    })
}

fn final_guarded_mode_packet(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(7).position(|w| {
        matches!(
            w,
            [
                Instruction::AddImmediateShifted {
                    immediate: -768,
                    ..
                },
                Instruction::AddImmediateShifted { immediate: -3, .. },
                Instruction::AddImmediate { immediate: -1, .. },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::AddImmediate {
                    immediate: -898,
                    ..
                },
                Instruction::StoreWord { offset: 4, .. },
                Instruction::AddImmediate { immediate: 8, .. },
            ]
        )
    })
}

fn final_terminal_object_packet(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(8).position(|w| {
        matches!(
            w,
            [
                Instruction::AddImmediateShifted {
                    immediate: 2816,
                    ..
                },
                Instruction::AddImmediate { immediate: 12, .. },
                Instruction::StoreWord { offset: 0, .. },
                Instruction::AddImmediateShifted {
                    immediate: 2304,
                    ..
                },
                Instruction::StoreWord { offset: 4, .. },
                Instruction::StoreWord { offset: 8, .. },
                Instruction::StoreWord { offset: 12, .. },
                Instruction::AddImmediate { immediate: 16, .. },
            ]
        )
    })
}

fn set_shifted_destination(instruction: &mut Instruction, register: u8) {
    let Instruction::AddImmediateShifted { d, .. } = instruction else {
        unreachable!()
    };
    *d = register;
}
fn set_add_low(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::AddImmediate { d, a, .. } = instruction else {
        unreachable!()
    };
    *d = destination;
    *a = base;
}
fn set_store_source(instruction: &mut Instruction, register: u8) {
    let Instruction::StoreWord { s, .. } = instruction else {
        unreachable!()
    };
    *s = register;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_final_guarded_mode_packet() {
        let instructions = vec![
            Instruction::load_immediate_shifted(10, -768),
            Instruction::load_immediate_shifted(11, -3),
            Instruction::AddImmediate {
                d: 0,
                a: 10,
                immediate: -1,
            },
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 11,
                immediate: -898,
            },
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
        ];
        assert_eq!(final_guarded_mode_packet(&instructions), Some(0));
    }
}
