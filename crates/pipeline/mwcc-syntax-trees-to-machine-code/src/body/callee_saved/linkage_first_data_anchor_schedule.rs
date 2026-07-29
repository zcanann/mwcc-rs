//! Physical scheduling for a linkage-first `.data` anchor frame.
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
            || !is_four_home_anchor_prefix(&self.output.instructions, self.frame_size)
        {
            return;
        }

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
}
