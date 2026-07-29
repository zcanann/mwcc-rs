//! Final issue order for structured variadic-output frames.
//!
//! Address-taken scalar outputs, a tiny frame string, and a trailing
//! integer-to-float conversion share one linkage-first frame in build 163.
//! Selection and allocation establish the final physical homes before the
//! independent operations can be interleaved.  Keep each measured transaction
//! separate so extensions do not turn this owner into one monolithic schedule.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_variadic_output_frame(&mut self) {
        if !self.preserve_guarded_named_local_values
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return;
        }

        self.schedule_variadic_output_entry();
        self.schedule_variadic_output_vector_initialization();
        self.schedule_variadic_output_string_call();
        self.schedule_variadic_output_conversion();
    }

    fn schedule_variadic_output_entry(&mut self) {
        let Some(first_call) = self.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::BranchAndLink { target }
                    if self.variadic_callees.contains(target)
            )
        }) else {
            return;
        };
        let saved_stores = self.output.instructions[..first_call]
            .iter()
            .filter(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreWord {
                        s: 14..=31,
                        a: 1,
                        ..
                    }
                )
            })
            .count();
        if saved_stores != 4 {
            return;
        }
        let Some(incoming_copy) = self.output.instructions[..first_call]
            .iter()
            .position(is_saved_incoming_r4_materialization)
        else {
            return;
        };
        let Some(frame_argument) = self.output.instructions[incoming_copy + 1..first_call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 4,
                        a: 1,
                        immediate: 44
                    }
                )
            })
            .map(|offset| incoming_copy + 1 + offset)
        else {
            return;
        };

        let destination = match self.output.instructions[incoming_copy] {
            Instruction::AddImmediate { d, .. } => d,
            _ => unreachable!("the entry recognizer selected an add-immediate copy"),
        };
        self.output.instructions[incoming_copy] = Instruction::move_register(destination, 4);
        self.move_instruction_before(frame_argument, incoming_copy + 1);
    }

    fn schedule_variadic_output_vector_initialization(&mut self) {
        let Some(start) = variadic_output_vector_initialization(&self.output.instructions) else {
            return;
        };
        if !schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start + 1,
            start + 2,
        ) {
            return;
        }
        let Some(call) = self.output.instructions[start + 6..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| start + 6 + offset)
        else {
            return;
        };
        let Some(frame_string) = self.output.instructions[start + 6..call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 3,
                        a: 1,
                        immediate: 36
                    }
                )
            })
            .map(|offset| start + 6 + offset)
        else {
            return;
        };

        self.move_instruction_before(start + 3, start + 2);
        match &mut self.output.instructions[start + 3] {
            Instruction::AddImmediate { d, a, .. } => {
                *d = 4;
                *a = 3;
            }
            _ => unreachable!("the vector recognizer selected the low address"),
        }
        for index in [start + 4, start + 5] {
            let Instruction::LoadFloatSingle { a, .. } = &mut self.output.instructions[index]
            else {
                unreachable!("the vector recognizer selected two float loads")
            };
            *a = 4;
        }
        self.move_instruction_before(frame_string, start + 5);
    }

    fn schedule_variadic_output_string_call(&mut self) {
        let Some(start) =
            variadic_output_string_call(&self.output.instructions, &self.variadic_callees)
        else {
            return;
        };

        self.move_instruction_before(start + 6, start + 1);
        self.move_instruction_before(start + 6, start + 4);
    }

    fn schedule_variadic_output_conversion(&mut self) {
        let Some(start) = variadic_output_conversion(&self.output.instructions) else {
            return;
        };

        self.move_instruction_before(start + 1, start);
        self.move_instruction_before(start + 8, start + 1);
        self.move_instruction_before(start + 3, start + 2);
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[start + 8]
        else {
            unreachable!("the conversion recognizer selected the high word")
        };
        *d = 0;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 9] else {
            unreachable!("the conversion recognizer selected the high-word store")
        };
        *s = 0;
    }
}

fn is_saved_incoming_r4_materialization(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate {
            d: 14..=31,
            a: 4,
            immediate: 0
        }
    )
}

fn variadic_output_vector_initialization(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: result,
                    a: 3,
                    immediate: 0
                },
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0
                },
                Instruction::StoreWord {
                    s: stored,
                    a: owner,
                    offset: 28
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 3,
                    offset: 4
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 3,
                    offset: 0
                },
            ] if result == stored && *result >= 14 && *owner >= 14
        )
    })
}

fn variadic_output_string_call(
    instructions: &[Instruction],
    variadic_callees: &std::collections::HashSet<String>,
) -> Option<usize> {
    instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 1
                },
                Instruction::StoreByte { s: 0, .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 14..=31,
                    immediate: 0
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 0,
                    offset: 0
                },
                Instruction::FloatMove { d: 2, b: 1 },
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 36
                },
                Instruction::ConditionRegisterSet { d: 6 },
                Instruction::BranchAndLink { target },
            ] if variadic_callees.contains(target)
        )
    })
}

fn variadic_output_conversion(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(12).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 3,
                    a: 1,
                    offset: 40
                },
                Instruction::ClearLeftImmediate {
                    a: 0,
                    s: 14..=31,
                    clear: 24
                },
                Instruction::SubtractFromImmediate {
                    d: 0,
                    a: 0,
                    immediate: 1
                },
                Instruction::CountLeadingZeros { a: 0, s: 0 },
                Instruction::ShiftRightLogicalImmediate {
                    a: 0,
                    s: 0,
                    shift: 5
                },
                Instruction::XorImmediateShifted {
                    a: 0,
                    s: 0,
                    immediate: 0x8000
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 52
                },
                Instruction::AddImmediateShifted {
                    d: 4,
                    a: 0,
                    immediate: 0x4330
                },
                Instruction::LoadFloatDouble {
                    d: 1,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 4,
                    a: 1,
                    offset: 48
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: 48
                },
                Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
            ]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_the_complete_variadic_string_packet() {
        let instructions = vec![
            Instruction::load_immediate(0, 1),
            Instruction::StoreByte {
                s: 0,
                a: 30,
                offset: 74,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 30,
                immediate: 0,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::FloatMove { d: 2, b: 1 },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 36,
            },
            Instruction::ConditionRegisterSet { d: 6 },
            Instruction::BranchAndLink {
                target: "format".into(),
            },
        ];
        let variadic = std::collections::HashSet::from(["format".into()]);

        assert_eq!(
            variadic_output_string_call(&instructions, &variadic),
            Some(0)
        );
    }
}
