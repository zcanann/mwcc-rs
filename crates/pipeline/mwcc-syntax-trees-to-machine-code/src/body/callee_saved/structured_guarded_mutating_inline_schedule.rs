//! Scheduling for a guarded mutating inline with caller-owned scratch storage.
//!
//! Build 163 retains the inlined receiver as the shared object value, saves the
//! later attribute home before the incoming parameters, and fills the two call
//! setup regions from independent integer and float operations.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_guarded_mutating_inline(&mut self, function: &Function) {
        if self.inline_statement_body_substitutions == 0
            || self.legacy_inline_expansion_frame_bytes == 0
            || !function.locals.iter().any(|local| {
                local.array_length.is_some()
                    && !super::structured_locals::body_uses_local(&function.statements, &local.name)
            })
        {
            return;
        }
        let Some(plan) = guarded_mutating_inline(&self.output.instructions) else {
            return;
        };

        if let Instruction::LoadWord { a, .. } = &mut self.output.instructions[plan.body_start + 1]
        {
            *a = plan.inlined_receiver;
        }
        self.output.instructions[plan.body_start + 8] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT,
            a: plan.inlined_receiver,
            immediate: 0,
        };

        // The inlined receiver owns the shared member load. Fill its dependent
        // attribute latency with zero, then copy the first call argument before
        // issuing the four independent stores.
        self.remove_structured_condition_instruction(plan.body_start);
        self.move_instruction_before(plan.body_start + 1, plan.body_start);
        self.move_instruction_before(plan.body_start + 2, plan.body_start + 1);
        self.move_instruction_before(plan.body_start + 7, plan.body_start + 3);

        // lfs f1; mr receiver; lfs f2; mr state; fmr f3,f1; li flags; li rate
        let final_call_setup = plan.body_start + 9;
        self.move_instruction_before(final_call_setup + 3, final_call_setup);
        self.move_instruction_before(final_call_setup + 4, final_call_setup + 2);
        self.move_instruction_before(final_call_setup + 5, final_call_setup + 4);

        // The attribute value is live across both calls. Legacy MWCC saves its
        // uninitialized home before the two incoming-parameter homes.
        let first_saved_home = self.output.instructions[..plan.body_start]
            .iter()
            .position(is_saved_home_store);
        let attribute_home =
            self.output.instructions[..plan.body_start]
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::StoreWord {
                            s,
                            a: 1,
                            ..
                        } if *s == plan.attributes
                    )
                });
        if let (Some(first_saved_home), Some(attribute_home)) = (first_saved_home, attribute_home) {
            if attribute_home > first_saved_home {
                self.move_instruction_before(attribute_home, first_saved_home);
            }
        }

        self.output.anonymous_label_bump += 6;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuardedMutatingInline {
    body_start: usize,
    inlined_receiver: u8,
    attributes: u8,
}

fn guarded_mutating_inline(instructions: &[Instruction]) -> Option<GuardedMutatingInline> {
    instructions
        .windows(18)
        .enumerate()
        .find_map(|(body_start, window)| match window {
            [
                Instruction::LoadWord {
                    d: outer_receiver,
                    a: outer_base,
                    offset: outer_offset,
                },
                Instruction::LoadWord {
                    d: attributes,
                    a: attribute_base,
                    ..
                },
                Instruction::LoadWord {
                    d: inlined_receiver,
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
                    s: copied_outer,
                    b: copied_outer_again,
                },
                Instruction::BranchAndLink { .. },
                Instruction::Or {
                    a: 3,
                    s: final_receiver,
                    b: final_receiver_again,
                },
                Instruction::Or {
                    a: 4,
                    s: final_state,
                    b: final_state_again,
                },
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
            ] if *outer_receiver != *inlined_receiver
                && *outer_base == *inlined_base
                && *outer_offset == *inlined_offset
                && *attribute_base == *outer_receiver
                && *attributes == *float_base
                && *inlined_receiver == *first_store
                && *inlined_receiver == *second_store
                && *inlined_receiver == *third_store
                && *inlined_receiver == *fourth_store
                && *outer_receiver == *copied_outer
                && *outer_receiver == *copied_outer_again
                && *final_receiver == *final_receiver_again
                && *final_state == *final_state_again =>
            {
                Some(GuardedMutatingInline {
                    body_start,
                    inlined_receiver: *inlined_receiver,
                    attributes: *attributes,
                })
            }
            _ => None,
        })
}

fn is_saved_home_store(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::StoreWord { s, a: 1, .. } if *s >= 32
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_shared_receiver_around_an_attribute_load() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 35,
                a: 33,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 34,
                a: 35,
                offset: 724,
            },
            Instruction::LoadWord {
                d: 36,
                a: 33,
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
                a: 36,
                offset,
            }),
        );
        instructions.extend([
            Instruction::Or { a: 3, s: 35, b: 35 },
            Instruction::BranchAndLink {
                target: "first".into(),
            },
            Instruction::Or { a: 3, s: 33, b: 33 },
            Instruction::Or { a: 4, s: 32, b: 32 },
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
                a: 34,
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

        assert_eq!(
            guarded_mutating_inline(&instructions),
            Some(GuardedMutatingInline {
                body_start: 0,
                inlined_receiver: 36,
                attributes: 34,
            })
        );
    }
}
