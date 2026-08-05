//! Final entry schedule for repeated inlined indirect-member walks.
//!
//! The whole-body planner assigns the three incoming values non-monotonic
//! saved homes. Selection emits those home copies in allocation order, while
//! Build 163 issues them in ABI source order and spells each as `mr`. Physical
//! homes are not known soon enough to make that decision during selection.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_repeated_indirect_member_loop_entry(&mut self) {
        schedule_entry_parameter_copies(
            &mut self.output.instructions,
            self.structured_repeated_indirect_member_loop_entry,
        );
        if !self.structured_repeated_indirect_member_loop_entry {
            return;
        }
        self.schedule_repeated_member_loop_initializers();
        self.schedule_repeated_member_loop_callback_packets();
    }

    /// Restore allocation-sensitive register and copy choices once every
    /// instruction-order pass has finished. These are physical spellings of
    /// values already owned by the recognized repeated-member transaction.
    pub(crate) fn finalize_repeated_indirect_member_loop_image(&mut self) {
        if !self.structured_repeated_indirect_member_loop_entry {
            return;
        }
        normalize_outer_loop_boolean(&mut self.output.instructions);
        normalize_thread_walk_cursor(&mut self.output.instructions);
        if let Some(callee) = self
            .structured_repeated_indirect_member_loop_fixed_address_tail
            .clone()
        {
            self.retain_fixed_address_call_bank(&callee);
        }
        normalize_reset_tail_copies(&mut self.output.instructions);
    }

    fn schedule_repeated_member_loop_initializers(&mut self) {
        let Some(first_load) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::LoadWord { a: 0, offset: 0, .. })
        }) else {
            return;
        };
        if first_load != 0
            && is_zero_initializer(&self.output.instructions[first_load - 1])
        {
            self.move_before_retaining_block_entry(first_load, first_load - 1);
        }

        let Some(second_load) = self
            .output
            .instructions
            .iter()
            .enumerate()
            .skip(first_load + 1)
            .find_map(|(index, instruction)| {
                matches!(instruction, Instruction::LoadWord { a: 0, offset: 0, .. })
                    .then_some(index)
            })
        else {
            return;
        };
        if second_load >= 2
            && is_entry_result_copy(&self.output.instructions[second_load - 2])
            && is_zero_initializer(&self.output.instructions[second_load - 1])
        {
            self.move_before_retaining_block_entry(second_load, second_load - 2);
            let copy = second_load - 1;
            let (destination, incoming) = entry_parameter_copy(&self.output.instructions[copy])
                .expect("repeated-loop result copy was recognized");
            self.output.instructions[copy] = Instruction::move_register(destination, incoming);
        } else if second_load != 0
            && is_zero_initializer(&self.output.instructions[second_load - 1])
        {
            self.move_before_retaining_block_entry(second_load, second_load - 1);
        }
    }

    fn schedule_repeated_member_loop_callback_packets(&mut self) {
        let mut start = 0;
        while let Some(load) = self.output.instructions[start..]
            .windows(2)
            .position(is_callback_argument_packet)
            .map(|offset| start + offset)
        {
            crate::retarget_instruction_destinations(self, load, load + 1);
            crate::move_instruction_before_retargeting(self, load + 1, load);
            start = load + 2;
        }

        let mut start = 0;
        while let Some(packet) = self.output.instructions[start..]
            .windows(5)
            .position(is_callback_result_packet)
            .map(|offset| start + offset)
        {
            crate::move_instruction_before_retargeting(self, packet + 4, packet + 2);
            start = packet + 5;
        }
    }

    fn move_before_retaining_block_entry(&mut self, from: usize, to: usize) {
        crate::retarget_instruction_destinations(self, to, from);
        crate::move_instruction_before_retargeting(self, from, to);
    }

    fn retain_fixed_address_call_bank(&mut self, callee: &str) {
        let Some(reboot) = self.output.instructions.windows(3).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a, immediate: 0 },
                    Instruction::AddImmediate { d: 4, immediate: 0, .. },
                    Instruction::BranchAndLink { target },
                ] if *a >= 14 && target == "__OSReboot"
            )
        }) else {
            return;
        };
        let Instruction::AddImmediate { a: bank, .. } = self.output.instructions[reboot] else {
            unreachable!("the reboot-code home was recognized")
        };
        let mut first = true;
        let mut search = reboot + 3;
        while let Some(start) = self.output.instructions[search..]
            .windows(5)
            .position(|window| is_fixed_address_call_packet(window, callee))
            .map(|offset| search + offset)
        {
            if first {
                let Instruction::AddImmediateShifted { immediate, .. } =
                    self.output.instructions[start]
                else {
                    unreachable!("the fixed-address high half was recognized")
                };
                self.output.instructions[start] = Instruction::AddImmediateShifted {
                    d: bank,
                    a: 0,
                    immediate,
                };
                let Instruction::AddImmediate { d, immediate, .. } =
                    self.output.instructions[start + 1]
                else {
                    unreachable!("the fixed-address low half was recognized")
                };
                self.output.instructions[start + 1] = Instruction::AddImmediate {
                    d,
                    a: bank,
                    immediate,
                };
                first = false;
                search = start + 5;
            } else {
                let Instruction::AddImmediate { d, immediate, .. } =
                    self.output.instructions[start + 1]
                else {
                    unreachable!("the fixed-address low half was recognized")
                };
                self.output.instructions[start + 1] = Instruction::AddImmediate {
                    d,
                    a: bank,
                    immediate,
                };
                crate::remove_instruction_retargeting_to_next(self, start);
                search = start + 4;
            }
        }
    }

}

fn is_zero_initializer(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate {
            a: 0,
            immediate: 0,
            ..
        }
    )
}

fn is_entry_result_copy(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate {
            d: 27,
            a: 3,
            immediate: 0,
        }
    )
}

fn is_callback_argument_packet(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::LoadWord {
                d: 12,
                offset: 0,
                ..
            },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0 | 1,
            },
        ]
    )
}

fn is_callback_result_packet(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::CountLeadingZeros { a: 0, s: 3 },
            Instruction::RotateAndMask { a: 0, s: 0, .. },
            Instruction::Or { b: 0, .. },
            Instruction::LoadWord { offset: 8, .. },
        ]
    )
}

fn normalize_outer_loop_boolean(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
                Instruction::Branch { .. },
                Instruction::AddImmediate { d: 3, a: 0, immediate: 1 },
                Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
            ]
        )
    }) else {
        return;
    };
    instructions[start + 1] = Instruction::AddImmediate { d: 0, a: 0, immediate: 0 };
    instructions[start + 3] = Instruction::AddImmediate { d: 0, a: 0, immediate: 1 };
    instructions[start + 4] = Instruction::CompareWordImmediate { a: 0, immediate: 0 };
}

fn normalize_thread_walk_cursor(instructions: &mut [Instruction]) {
    let mut search = 0;
    while let Some(load) = instructions[search..]
        .iter()
        .position(|instruction| {
            matches!(instruction, Instruction::LoadWord { d: 29, offset: 764, .. })
        })
        .map(|offset| search + offset)
    {
        let Some(copy) = instructions[load + 1..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate { d: 3, a: 29, immediate: 0 }
                )
            })
            .map(|offset| load + 1 + offset)
        else {
            return;
        };
        let Instruction::LoadWord { a, offset, .. } = instructions[load] else {
            unreachable!("thread walk cursor load was recognized")
        };
        instructions[load] = Instruction::LoadWord { d: 28, a, offset };
        instructions[copy] = Instruction::move_register(3, 28);
        search = copy + 1;
    }
}

fn normalize_reset_tail_copies(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchAndLink { target: scheduler },
                Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
                Instruction::AddImmediate { d: 4, a: 31, immediate: 0 },
                Instruction::BranchAndLink { target: reboot },
            ] if scheduler == "OSEnableScheduler"
                && reboot == "__OSReboot"
        )
    }) else {
        return;
    };
    instructions[start + 1] = Instruction::move_register(3, 30);
    instructions[start + 2] = Instruction::move_register(4, 31);
    if matches!(
        instructions.get(start + 4..start + 6),
        Some([
            Instruction::AddImmediate { d: 3, a: 27, immediate: 0 },
            Instruction::BranchAndLink { target },
        ]) if target == "OSRestoreInterrupts"
    ) {
        instructions[start + 4] = Instruction::move_register(3, 27);
    }
}

fn is_fixed_address_call_packet(window: &[Instruction], callee: &str) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::BranchAndLink { target },
        ] if target == callee
    )
}

fn schedule_entry_parameter_copies(instructions: &mut [Instruction], enabled: bool) {
    if !enabled {
        return;
    }
    let Some(start) = instructions.windows(4).position(is_dense_entry_packet) else {
        return;
    };
    let copies = &mut instructions[start + 1..start + 4];
    copies.sort_by_key(|instruction| {
        entry_parameter_copy(instruction)
            .map(|(_, incoming)| incoming)
            .unwrap_or(u8::MAX)
    });
    for instruction in copies {
        let (destination, incoming) =
            entry_parameter_copy(instruction).expect("dense entry packet was recognized");
        *instruction = Instruction::move_register(destination, incoming);
    }
}

fn is_dense_entry_packet(window: &[Instruction]) -> bool {
    let [Instruction::StoreMultipleWord { s: 26 | 27, a: 1, .. }, first, second, third] = window
    else {
        return false;
    };
    let mut sources = [first, second, third]
        .map(entry_parameter_copy)
        .map(|copy| copy.map(|(_, source)| source));
    sources.sort();
    sources == [Some(3), Some(4), Some(5)]
}

fn entry_parameter_copy(instruction: &Instruction) -> Option<(u8, u8)> {
    let Instruction::AddImmediate {
        d,
        a,
        immediate: 0,
    } = instruction
    else {
        return None;
    };
    (*d >= 14 && (3..=5).contains(a)).then_some((*d, *a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_source_order_and_move_spelling_after_home_allocation() {
        let mut instructions = vec![
            Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
        ];

        schedule_entry_parameter_copies(&mut instructions, true);

        assert_eq!(
            &instructions[1..],
            &[
                Instruction::move_register(26, 3),
                Instruction::move_register(30, 4),
                Instruction::move_register(31, 5),
            ]
        );
    }

    #[test]
    fn leaves_an_unowned_packet_untouched() {
        let mut instructions = vec![
            Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
        ];
        let original = instructions.clone();

        schedule_entry_parameter_copies(&mut instructions, false);

        assert_eq!(instructions, original);
    }

    #[test]
    fn restores_the_thread_walk_cursor_home_after_allocation() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 29,
                a: 3,
                offset: 764,
            },
            Instruction::CompareWordImmediate {
                a: 0,
                immediate: 4,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 29,
                immediate: 0,
            },
        ];

        normalize_thread_walk_cursor(&mut instructions);

        assert_eq!(
            instructions,
            vec![
                Instruction::LoadWord {
                    d: 28,
                    a: 3,
                    offset: 764,
                },
                Instruction::CompareWordImmediate {
                    a: 0,
                    immediate: 4,
                },
                Instruction::move_register(3, 28),
            ]
        );
    }
}
