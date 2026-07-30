//! Physical scheduling for a linkage-first writable-section anchor frame.
//!
//! Selection keeps the anchor, saved homes, and first variadic call as separate
//! semantic packets. Build 163 interleaves those packets across the linkage
//! stores, then orders the physical r31..r28 save suffix independently of the
//! homes' source-lifetime order.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_data_anchor_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.data_section_anchor.as_ref().and_then(|plan| plan.register).is_none()
        {
            return;
        }
        if is_four_home_anchor_prefix(&self.output.instructions, self.frame_size) {
            self.schedule_four_home_data_anchor_frame();
        } else if is_two_home_anchor_prefix(&self.output.instructions, self.frame_size) {
            self.schedule_two_home_data_anchor_frame();
        } else if is_anchor_only_prefix(&self.output.instructions, self.frame_size) {
            self.schedule_anchor_only_frame();
        }
        normalize_data_anchor_array_lookup(&mut self.output.instructions);
    }

    fn schedule_anchor_only_frame(&mut self) {
        // Build 163 starts materializing a retained writable-section base
        // between `mflr` and the linkage stores, then finishes it directly
        // into the sole saved register after that register has been saved.
        self.move_instruction_before(4, 1);
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[1] else {
            unreachable!("the anchor-only high half was matched")
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[5] else {
            unreachable!("the anchor-only low half was matched")
        };
        *a = 3;
    }

    fn schedule_four_home_data_anchor_frame(&mut self) {
        // mflr; lis anchor; stw LR; crclr; li -1; stwu; stfd; stw r31;
        // addi anchor; li 11; stw r30; stw r29; copy r29; frame address;
        // stw r28; copy r28; load retained object.
        self.move_instruction_before(8, 1);
        for (from, to) in [
            (3, 2),
            (4, 3),
            (5, 4),
            (6, 5),
            (7, 6),
            (8, 7),
            (9, 8),
        ] {
            self.move_instruction_before(from, to);
        }
        self.move_instruction_before(12, 10);
        self.move_instruction_before(15, 13);

        let frame_size = self.frame_size;
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset,
                } if *offset == frame_size - 20 => *offset = frame_size - 16,
                Instruction::StoreWord {
                    s: 29,
                    a: 1,
                    offset,
                } if *offset == frame_size - 16 => *offset = frame_size - 20,
                Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset,
                } if *offset == frame_size - 20 => *offset = frame_size - 16,
                Instruction::LoadWord {
                    d: 29,
                    a: 1,
                    offset,
                } if *offset == frame_size - 16 => *offset = frame_size - 20,
                _ => {}
            }
        }
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[16] else {
            unreachable!("the anchor prefix retained-object load was matched")
        };
        *a = 3;
        if let Some(first_restore) = self.output.instructions.windows(2).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord {
                        d: 29,
                        a: 1,
                        offset: first,
                    },
                    Instruction::LoadWord {
                        d: 30,
                        a: 1,
                        offset: second,
                    },
                ] if *first == frame_size - 20 && *second == frame_size - 16
            )
        }) {
            self.move_instruction_before(first_restore + 1, first_restore);
        }

        normalize_terminal_saved_parameter_forward(&mut self.output.instructions);
    }

    fn schedule_two_home_data_anchor_frame(&mut self) {
        // Save and derive the retained receiver before redefining r3 as the
        // section-base staging lane.
        self.move_instruction_before(6, 4);
        self.move_instruction_before(7, 5);
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[6] else {
            unreachable!("the two-home data-anchor high half was matched")
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[7] else {
            unreachable!("the two-home data-anchor low half was matched")
        };
        *a = 3;

        if let Some(start) = first_two_home_float_call_arguments(&self.output.instructions) {
            self.move_instruction_before(start + 2, start);
            self.move_instruction_before(start + 3, start + 2);
        }
        if let Some((from, to)) =
            two_home_first_result_initialization(&self.output.instructions)
        {
            self.move_instruction_before(from, to);
        }
        if let Some(copy) = two_home_result_copy(&self.output.instructions) {
            self.output.instructions[copy] = Instruction::move_register(31, 3);
        }
        if let Some((frame_argument, first_initializer)) =
            two_home_frame_argument_schedule(&self.output.instructions)
        {
            self.move_instruction_before(frame_argument, first_initializer);
        }
        if let Some(start) = two_home_terminal_variadic_arguments(&self.output.instructions) {
            self.move_instruction_before(start + 6, start + 1);
            self.move_instruction_before(start + 6, start + 4);
        }
    }
}

fn is_two_home_anchor_prefix(instructions: &[Instruction], frame_size: i16) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord { s: 30, a: 1, .. },
            Instruction::LoadWord {
                d: 30,
                a: 3,
                offset: 44,
            },
            ..
        ] if *offset == -frame_size
    )
}

fn is_anchor_only_prefix(instructions: &[Instruction], frame_size: i16) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            ..
        ] if *offset == -frame_size
    )
}

fn first_two_home_float_call_arguments(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle { d: 1, a: 31, .. },
                Instruction::LoadFloatSingle { d: 2, a: 31, .. },
            ]
        )
    })
}

fn two_home_first_result_initialization(instructions: &[Instruction]) -> Option<(usize, usize)> {
    if let Some(start) = instructions.windows(7).position(|window| {
        matches!(
            window,
            [
                Instruction::StoreWord {
                    s: 3,
                    a: 30,
                    offset: 20,
                },
                Instruction::LoadFloatSingle { d: 0, .. },
                Instruction::StoreFloatSingle {
                    s: 0,
                    a: 3,
                    offset: 36,
                },
                Instruction::LoadFloatSingle { d: 0, .. },
                Instruction::StoreFloatSingle {
                    s: 0,
                    a: 3,
                    offset: 40,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 2,
                },
                Instruction::StoreByte {
                    s: 0,
                    a: 3,
                    offset: 74,
                },
            ]
        )
    }) {
        return Some((start + 5, start + 1));
    }
    instructions
        .windows(6)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::StoreWord {
                        s: 3,
                        a: 30,
                        offset: 20,
                    },
                    Instruction::LoadFloatSingle { d: 0, .. },
                    Instruction::StoreFloatSingle {
                        s: 0,
                        a: 3,
                        offset: 36,
                    },
                    Instruction::StoreFloatSingle {
                        s: 0,
                        a: 3,
                        offset: 40,
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 2,
                    },
                    Instruction::StoreByte {
                        s: 0,
                        a: 3,
                        offset: 74,
                    },
                ]
            )
        })
        .map(|start| (start + 4, start + 1))
}

fn two_home_frame_argument_schedule(instructions: &[Instruction]) -> Option<(usize, usize)> {
    let result_store = instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWord {
                s: 31,
                a: 30,
                offset: 24,
            }
        )
    })?;
    let tail = instructions.get(result_store + 1..)?;
    let frame_argument = tail.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 20,
            }
        )
    })? + result_store
        + 1;
    let first_initializer = result_store + 1;
    matches!(
        instructions.get(first_initializer),
        Some(Instruction::LoadFloatSingle { d: 0, .. })
    )
    .then_some((frame_argument, first_initializer))
}

fn two_home_result_copy(instructions: &[Instruction]) -> Option<usize> {
    instructions.iter().rposition(|instruction| {
        matches!(
            instruction,
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            }
        )
    })
}

fn two_home_terminal_variadic_arguments(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 1,
                },
                Instruction::StoreByte {
                    s: 0,
                    a: 31,
                    offset: 74,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 31,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle { d: 1, .. },
                Instruction::FloatMove { d: 2, b: 1 },
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 20,
                },
                Instruction::ConditionRegisterSet { d: 6 },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

fn is_four_home_anchor_prefix(instructions: &[Instruction], frame_size: i16) -> bool {
    let Some(prefix) = instructions.get(..19) else {
        return false;
    };
    matches!(
        prefix,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 11,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            _,
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset,
            },
            Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                ..
            },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord { s: 29, a: 1, .. },
            Instruction::AddImmediate {
                d: 29,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord { s: 30, a: 1, .. },
            Instruction::StoreWord { s: 28, a: 1, .. },
            Instruction::AddImmediate {
                d: 28,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 30,
                a: 28,
                offset: 40,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 30,
                immediate: 0,
            },
            Instruction::BranchAndLink { .. },
        ] if *offset == -frame_size
    )
}

fn normalize_terminal_saved_parameter_forward(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(3).rposition(|window| {
        matches!(
            window,
            [
                Instruction::Or {
                    a: 3,
                    s: first,
                    b,
                },
                Instruction::ClearLeftImmediate {
                    a: 4,
                    s: second,
                    clear: 24,
                },
                Instruction::BranchAndLink { .. },
            ] if first == b && (14..=31).contains(first) && (14..=31).contains(second)
        )
    }) else {
        return;
    };
    let (first, second) = match (&instructions[start], &instructions[start + 1]) {
        (
            Instruction::Or { s: first, .. },
            Instruction::ClearLeftImmediate { s: second, .. },
        ) => (*first, *second),
        _ => unreachable!("the terminal saved-parameter forward was matched"),
    };
    instructions[start] = Instruction::AddImmediate {
        d: 3,
        a: first,
        immediate: 0,
    };
    instructions[start + 1] = Instruction::AddImmediate {
        d: 4,
        a: second,
        immediate: 0,
    };
}

fn normalize_data_anchor_array_lookup(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadByteZero {
                    d: index,
                    a: root,
                    ..
                },
                Instruction::Add {
                    d: first_address,
                    a: anchor,
                    b: first_index,
                },
                Instruction::LoadByteZero {
                    d: value,
                    a: first_base,
                    ..
                },
                Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: shifted,
                    shift: 3,
                },
                Instruction::Add {
                    d: second_address,
                    a: second_anchor,
                    b: second_index,
                },
                Instruction::LoadHalfwordZero {
                    d: result,
                    a: second_base,
                    ..
                },
            ] if index == first_address
                && index == first_index
                && first_address == value
                && first_address == first_base
                && value == scaled
                && value == shifted
                && scaled == second_address
                && scaled == second_index
                && second_address == result
                && second_address == second_base
                && anchor == second_anchor
                && *anchor == 31
                && *root == 30
        )
    }) else {
        return;
    };
    let Instruction::LoadByteZero { d, .. } = &mut instructions[start] else {
        unreachable!("the data-anchor byte-table index was matched")
    };
    *d = GENERAL_SCRATCH;
    let Instruction::Add { b, .. } = &mut instructions[start + 1] else {
        unreachable!("the first data-anchor table address was matched")
    };
    *b = GENERAL_SCRATCH;
    let Instruction::LoadByteZero { d, .. } = &mut instructions[start + 2] else {
        unreachable!("the data-anchor byte-table load was matched")
    };
    *d = GENERAL_SCRATCH;
    let Instruction::ShiftLeftImmediate { a, s, .. } = &mut instructions[start + 3] else {
        unreachable!("the data-anchor table scale was matched")
    };
    *a = GENERAL_SCRATCH;
    *s = GENERAL_SCRATCH;
    let Instruction::Add { b, .. } = &mut instructions[start + 4] else {
        unreachable!("the second data-anchor table address was matched")
    };
    *b = GENERAL_SCRATCH;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_terminal_saved_parameter_copies() {
        let mut instructions = vec![
            Instruction::move_register(3, 28),
            Instruction::ClearLeftImmediate {
                a: 4,
                s: 29,
                clear: 24,
            },
            Instruction::BranchAndLink {
                target: "tail".into(),
            },
        ];

        normalize_terminal_saved_parameter_forward(&mut instructions);

        assert!(matches!(
            instructions.as_slice(),
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 28,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 29,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ]
        ));
    }

    #[test]
    fn uses_scratch_for_chained_data_anchor_array_indices() {
        let mut instructions = vec![
            Instruction::LoadByteZero {
                d: 4,
                a: 30,
                offset: 1,
            },
            Instruction::Add {
                d: 4,
                a: 31,
                b: 4,
            },
            Instruction::LoadByteZero {
                d: 4,
                a: 4,
                offset: 104,
            },
            Instruction::ShiftLeftImmediate {
                a: 4,
                s: 4,
                shift: 3,
            },
            Instruction::Add {
                d: 4,
                a: 31,
                b: 4,
            },
            Instruction::LoadHalfwordZero {
                d: 4,
                a: 4,
                offset: 184,
            },
        ];

        normalize_data_anchor_array_lookup(&mut instructions);

        assert!(matches!(
            instructions.as_slice(),
            [
                Instruction::LoadByteZero { d: 0, a: 30, .. },
                Instruction::Add {
                    d: 4,
                    a: 31,
                    b: 0,
                },
                Instruction::LoadByteZero { d: 0, a: 4, .. },
                Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: 0,
                    shift: 3,
                },
                Instruction::Add {
                    d: 4,
                    a: 31,
                    b: 0,
                },
                Instruction::LoadHalfwordZero { d: 4, a: 4, .. },
            ]
        ));
    }

    #[test]
    fn selects_the_late_result_copy_after_the_anchor_low_half() {
        let instructions = vec![
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "allocate".into(),
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
        ];

        assert_eq!(two_home_result_copy(&instructions), Some(2));
    }

    #[test]
    fn schedules_alignment_before_duplicate_literal_store_loads_are_reused() {
        let instructions = vec![
            Instruction::StoreWord {
                s: 3,
                a: 30,
                offset: 20,
            },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 36 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 40 },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 2,
            },
            Instruction::StoreByte {
                s: 0,
                a: 3,
                offset: 74,
            },
        ];

        assert_eq!(two_home_first_result_initialization(&instructions), Some((5, 1)));
    }
}
