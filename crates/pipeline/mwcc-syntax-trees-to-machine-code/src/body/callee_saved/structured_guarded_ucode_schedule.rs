//! Scheduling for two guarded display-list packets populated by direct calls.
//!
//! Macro-expanded ucode loading emits two consecutive eight-byte packets. MWCC
//! overlaps each command materialization with cursor publication and starts the
//! second command while the first call result is becoming available.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_guarded_ucode_packets(&mut self) {
        while let Some(packet) = guarded_ucode_packets(&self.output.instructions) {
            let start = packet.start;

            // First packet: command-high, packet alias, command store, cursor bump.
            self.move_instruction_before(start + 2, start);
            self.move_instruction_before(start + 3, start + 2);

            // Second packet: start its command-high before the first result store,
            // then finish the command around the packet alias and cursor bump.
            self.move_instruction_before(start + 8, start + 5);
            self.move_instruction_before(start + 9, start + 7);
            self.move_instruction_before(start + 10, start + 9);

            assign_guarded_ucode_packet_registers(
                &mut self.output.instructions[start..start + 13],
                packet.cursor,
            );
        }
    }

    pub(crate) fn finalize_structured_guarded_ucode_packet_registers(&mut self) {
        let packets = scheduled_guarded_ucode_packets(&self.output.instructions);
        let Some(reusable_alias) = packets.first().map(|packet| packet.first_alias) else {
            return;
        };
        if packets.len() < 2 {
            return;
        }
        for packet in packets {
            for alias in [packet.start + 1, packet.start + 8] {
                let Instruction::Or { a, .. } = &mut self.output.instructions[alias] else {
                    unreachable!("the scheduled packet alias was matched")
                };
                *a = reusable_alias;
            }
            for result_store in [packet.start + 6, packet.start + 12] {
                let Instruction::StoreWord { a, .. } = &mut self.output.instructions[result_store]
                else {
                    unreachable!("the scheduled packet result store was matched")
                };
                *a = reusable_alias;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct GuardedUcodePackets {
    start: usize,
    cursor: u8,
}

fn guarded_ucode_packets(instructions: &[Instruction]) -> Option<GuardedUcodePackets> {
    instructions
        .windows(13)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                    window,
                    [
                        Instruction::Or {
                            a: first_packet,
                            s: first_cursor,
                            b: first_cursor_b,
                        },
                        Instruction::AddImmediate {
                            d: bumped_cursor,
                            a: bump_source,
                            immediate: 8,
                        },
                        Instruction::AddImmediateShifted {
                            d: first_command,
                            a: 0,
                            ..
                        },
                        Instruction::StoreWord {
                            s: first_stored_command,
                            a: first_store_base,
                            offset: 0,
                        },
                        Instruction::BranchAndLink { .. },
                        Instruction::StoreWord {
                            s: Eabi::FIRST_GENERAL_ARGUMENT,
                            a: first_result_base,
                            offset: 4,
                        },
                        Instruction::Or {
                            a: second_packet,
                            s: second_cursor,
                            b: second_cursor_b,
                        },
                        Instruction::AddImmediate {
                            d: second_bumped_cursor,
                            a: second_bump_source,
                            immediate: 8,
                        },
                        Instruction::AddImmediateShifted {
                            d: second_command_high,
                            a: 0,
                            ..
                        },
                        Instruction::AddImmediate {
                            d: second_command,
                            a: second_command_base,
                            ..
                        },
                        Instruction::StoreWord {
                            s: second_stored_command,
                            a: second_store_base,
                            offset: 0,
                        },
                        Instruction::BranchAndLink { .. },
                        Instruction::StoreWord {
                            s: Eabi::FIRST_GENERAL_ARGUMENT,
                            a: second_result_base,
                            offset: 4,
                        },
                    ] if first_packet != first_cursor
                        && first_cursor == first_cursor_b
                        && first_cursor == bumped_cursor
                        && first_cursor == bump_source
                        && first_command == first_stored_command
                        && first_packet == first_store_base
                        && first_packet == first_result_base
                        && second_packet != second_cursor
                        && second_cursor == first_cursor
                        && second_cursor == second_cursor_b
                        && second_cursor == second_bumped_cursor
                        && second_cursor == second_bump_source
                        && second_command_high == second_command_base
                        && second_command == second_stored_command
                        && second_packet == second_store_base
                        && second_packet == second_result_base
            )
            .then(|| GuardedUcodePackets {
                start,
                cursor: match window[0] {
                    Instruction::Or { s, .. } => s,
                    _ => unreachable!("the first packet alias was matched"),
                },
            })
        })
}

fn assign_guarded_ucode_packet_registers(instructions: &mut [Instruction], cursor: u8) {
    let Instruction::AddImmediateShifted { d, .. } = &mut instructions[0] else {
        unreachable!("the first command high was scheduled first")
    };
    *d = 0;
    let Instruction::StoreWord { s, a, .. } = &mut instructions[2] else {
        unreachable!("the first command store was scheduled third")
    };
    *s = 0;
    *a = cursor;

    let Instruction::AddImmediateShifted { d, .. } = &mut instructions[5] else {
        unreachable!("the second command high was scheduled before the first result")
    };
    *d = Eabi::FIRST_GENERAL_ARGUMENT + 1;
    let Instruction::AddImmediate { d, a, .. } = &mut instructions[7] else {
        unreachable!("the second command low was scheduled third")
    };
    *d = 0;
    *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;
    let Instruction::StoreWord { s, a, .. } = &mut instructions[9] else {
        unreachable!("the second command store was scheduled before the cursor bump")
    };
    *s = 0;
    *a = cursor;
}

#[derive(Clone, Copy)]
struct ScheduledGuardedUcodePackets {
    start: usize,
    first_alias: u8,
}

fn scheduled_guarded_ucode_packets(
    instructions: &[Instruction],
) -> Vec<ScheduledGuardedUcodePackets> {
    instructions
        .windows(13)
        .enumerate()
        .filter_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::AddImmediateShifted {
                        d: 0,
                        a: 0,
                        ..
                    },
                    Instruction::Or {
                        a: first_alias,
                        s: cursor,
                        b: first_cursor_b,
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: first_command_base,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: bumped_cursor,
                        a: bump_source,
                        immediate: 8,
                    },
                    Instruction::BranchAndLink { .. },
                    Instruction::AddImmediateShifted {
                        d: second_high,
                        a: 0,
                        ..
                    },
                    Instruction::StoreWord {
                        s: Eabi::FIRST_GENERAL_ARGUMENT,
                        a: first_result_base,
                        offset: 4,
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: second_high_base,
                        ..
                    },
                    Instruction::Or {
                        a: second_alias,
                        s: second_cursor,
                        b: second_cursor_b,
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: second_command_base,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: second_bumped_cursor,
                        a: second_bump_source,
                        immediate: 8,
                    },
                    Instruction::BranchAndLink { .. },
                    Instruction::StoreWord {
                        s: Eabi::FIRST_GENERAL_ARGUMENT,
                        a: second_result_base,
                        offset: 4,
                    },
                ] if first_alias != cursor
                    && first_alias == first_result_base
                    && cursor == first_cursor_b
                    && cursor == first_command_base
                    && cursor == bumped_cursor
                    && cursor == bump_source
                    && second_high == second_high_base
                    && second_alias != cursor
                    && second_alias == second_result_base
                    && cursor == second_cursor
                    && cursor == second_cursor_b
                    && cursor == second_command_base
                    && cursor == second_bumped_cursor
                    && cursor == second_bump_source
            )
            .then(|| ScheduledGuardedUcodePackets {
                start,
                first_alias: match window[1] {
                    Instruction::Or { a, .. } => a,
                    _ => unreachable!("the first scheduled alias was matched"),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_two_guarded_call_populated_packets() {
        let instructions = vec![
            Instruction::move_register(38, 37),
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
            Instruction::load_immediate_shifted(0, -7936),
            Instruction::StoreWord {
                s: 0,
                a: 38,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "get_data".into(),
            },
            Instruction::StoreWord {
                s: 3,
                a: 38,
                offset: 4,
            },
            Instruction::move_register(39, 37),
            Instruction::AddImmediate {
                d: 37,
                a: 37,
                immediate: 8,
            },
            Instruction::load_immediate_shifted(54, -8960),
            Instruction::AddImmediate {
                d: 0,
                a: 54,
                immediate: 2047,
            },
            Instruction::StoreWord {
                s: 0,
                a: 39,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "get_text".into(),
            },
            Instruction::StoreWord {
                s: 3,
                a: 39,
                offset: 4,
            },
        ];
        let packet = guarded_ucode_packets(&instructions).expect("the packet pair should match");
        assert_eq!((packet.start, packet.cursor), (0, 37));
    }
}
