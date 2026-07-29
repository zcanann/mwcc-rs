//! Receiver coalescing for a retained mutating statement-body inline.
//!
//! A caller-side pointer and the inline helper's hygienic pointer can carry the
//! same member load. Build 163 keeps the helper value, copies it into r3 after
//! the first store, and fills the following call setup from the literal loads.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_inlined_store_receiver(&mut self) {
        if self.inline_statement_body_substitutions == 0
            || self.legacy_inline_expansion_frame_bytes == 0
        {
            return;
        }
        let Some((start, outer, inlined)) = inlined_store_receiver(&self.output.instructions)
        else {
            return;
        };
        let duplicate_load = start;
        let argument_copy = start + 7;
        if self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if *target == duplicate_load
            )
        }) {
            return;
        }

        match &mut self.output.instructions[argument_copy] {
            Instruction::Or { s, b, .. } => {
                *s = inlined;
                *b = inlined;
            }
            _ => unreachable!("inlined store receiver was recognized"),
        }
        self.remove_structured_condition_instruction(duplicate_load);
        // load helper receiver; li zero; first store; mr r3,receiver; remaining stores
        self.move_instruction_before(start + 6, start + 3);

        // lfs f1; mr receiver; lfs f2; li state; fmr f3,f1; li flags; li rate; call
        let final_call = start + 8;
        self.move_instruction_before(final_call + 3, final_call);
        self.move_instruction_before(final_call + 4, final_call + 2);
        self.move_instruction_before(final_call + 5, final_call + 4);

        // The retained helper contributes its call-site boundary plus the
        // three store-value graph nodes visible before the two float literals.
        self.output.anonymous_label_bump += 6;
        debug_assert_ne!(outer, inlined);
    }
}

fn inlined_store_receiver(instructions: &[Instruction]) -> Option<(usize, u8, u8)> {
    instructions.windows(17).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::LoadWord {
                    d: outer,
                    a: outer_base,
                    offset: outer_offset,
                },
                Instruction::LoadWord {
                    d: inlined,
                    a: inlined_base,
                    offset: inlined_offset,
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
                Instruction::Or {
                    a: 3,
                    s: copied,
                    b: copied_again,
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
                Instruction::LoadFloatSingle { d: 2, .. },
                Instruction::FloatMove { d: 3, b: 1 },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if *outer != *inlined
                && *outer_base == *inlined_base
                && *outer_offset == *inlined_offset
                && *inlined == *first_store
                && *inlined == *second_store
                && *inlined == *third_store
                && *inlined == *fourth_store
                && *outer == *copied
                && *outer == *copied_again
                && *final_receiver == *final_receiver_again =>
            {
                Some((start, *outer, *inlined))
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_duplicate_caller_and_inline_receiver_loads() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 33,
                a: 32,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 34,
                a: 32,
                offset: 44,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
        ];
        instructions.extend(
            [8712, 8708, 8704, 8720].map(|offset| Instruction::StoreWord {
                s: 0,
                a: 34,
                offset,
            }),
        );
        instructions.extend([
            Instruction::Or { a: 3, s: 33, b: 33 },
            Instruction::BranchAndLink {
                target: "first".into(),
            },
            Instruction::Or { a: 3, s: 32, b: 32 },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 361,
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
                a: 0,
                offset: 0,
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

        assert_eq!(inlined_store_receiver(&instructions), Some((0, 33, 34)));
    }
}
