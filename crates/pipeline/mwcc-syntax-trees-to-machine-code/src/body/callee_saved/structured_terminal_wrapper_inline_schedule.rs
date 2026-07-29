//! Final schedule for a repeated guarded transaction in a terminal wrapper.
//!
//! The wrapper has one saved receiver and a literal state argument. Build 163
//! shares the inlined object load, fills its attribute latency with zero, and
//! overlaps the final integer and float call setup.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_terminal_wrapper_mutating_inline(&mut self, function: &Function) {
        if !function.locals.iter().any(|local| {
            local.array_length.is_some()
                && !super::structured_locals::body_uses_local(&function.statements, &local.name)
        }) {
            return;
        }
        let Some(start) = terminal_wrapper_mutating_inline(&self.output.instructions) else {
            return;
        };

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start] else {
            unreachable!("terminal wrapper inline changed form")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!("terminal wrapper attribute load changed form")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.output.instructions[start + 2] = Instruction::Or {
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            s: Eabi::FIRST_GENERAL_ARGUMENT + 1,
            b: Eabi::FIRST_GENERAL_ARGUMENT + 1,
        };

        // load receiver; li zero; load attributes; copy first call receiver.
        self.move_instruction_before(start + 3, start + 1);

        // lfs f1; mr receiver; lfs f2; li state; fmr f3,f1; li flags; li rate.
        let final_call_setup = start + 9;
        self.move_instruction_before(final_call_setup + 3, final_call_setup);
        self.move_instruction_before(final_call_setup + 4, final_call_setup + 2);
        self.move_instruction_before(final_call_setup + 5, final_call_setup + 4);
    }
}

fn terminal_wrapper_mutating_inline(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(17).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: outer_receiver,
                    a: saved_receiver,
                    offset: outer_offset,
                },
                Instruction::LoadWord {
                    d: attributes,
                    a: attribute_base,
                    ..
                },
                Instruction::LoadWord {
                    d: inlined_receiver,
                    a: duplicate_base,
                    offset: duplicate_offset,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: first_store,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: second_store,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: third_store,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: fourth_store,
                    ..
                },
                Instruction::BranchAndLink { .. },
                Instruction::Or {
                    a: 3,
                    s: final_receiver,
                    b: final_receiver_again,
                },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::LoadFloatSingle { d: 1, .. },
                Instruction::LoadFloatSingle {
                    d: 2,
                    a: float_base,
                    ..
                },
                Instruction::FloatMove { d: 3, b: 1 },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if *outer_receiver == 3
                && *inlined_receiver == 4
                && *outer_receiver != *inlined_receiver
                && *saved_receiver == *duplicate_base
                && *outer_offset == *duplicate_offset
                && *attribute_base == *outer_receiver
                && *attributes == *float_base
                && *inlined_receiver == *first_store
                && *inlined_receiver == *second_store
                && *inlined_receiver == *third_store
                && *inlined_receiver == *fourth_store
                && *saved_receiver == *final_receiver
                && *saved_receiver == *final_receiver_again
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_saved_receiver_and_literal_state_tail() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 30,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 724,
            },
            Instruction::LoadWord {
                d: 4,
                a: 30,
                offset: 44,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
        ];
        instructions.extend(
            [8712, 8708, 8704, 8720].map(|offset| Instruction::StoreWord { s: 0, a: 4, offset }),
        );
        instructions.extend([
            Instruction::BranchAndLink {
                target: "first".into(),
            },
            Instruction::Or { a: 3, s: 30, b: 30 },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 360,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 2,
                a: 31,
                offset: 128,
            },
            Instruction::FloatMove { d: 3, b: 1 },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "second".into(),
            },
        ]);

        assert_eq!(terminal_wrapper_mutating_inline(&instructions), Some(0));
    }
}
