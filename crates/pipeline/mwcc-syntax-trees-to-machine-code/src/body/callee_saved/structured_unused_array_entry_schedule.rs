//! Entry scheduling for a call sequence that carries an unused scratch array.
//!
//! With two saved values, build 163 batches their stores, leaves the incoming
//! receiver in r3 for the first call, and overlaps the second callback address
//! with publication of the first.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_unused_array_call_entry(&mut self, function: &Function) {
        if self.inline_statement_body_substitutions != 0
            || !function.locals.iter().any(|local| {
                local.array_length.is_some()
                    && !super::structured_locals::body_uses_local(&function.statements, &local.name)
            })
        {
            return;
        }
        let Some(start) = unused_array_call_entry(&self.output.instructions) else {
            return;
        };

        // save loaded object; save entry; copy entry; then initialize object.
        self.move_instruction_before(start + 2, start + 1);
        self.move_instruction_before(start + 3, start + 2);

        // r3 still holds the entry receiver: the four zero stores do not use it.
        self.remove_structured_condition_instruction(start + 9);

        // Start loading the second callback before publishing the first.
        self.move_instruction_before(start + 21, start + 20);
        self.output.anonymous_label_bump += 6;
    }

    /// Finish the same transaction after linkage normalization has hoisted the
    /// first call's integer arguments into the physical prologue.
    pub(crate) fn schedule_unused_array_call_linkage(&mut self, function: &Function) {
        if !function.locals.iter().any(|local| {
            local.array_length.is_some()
                && !super::structured_locals::body_uses_local(
                    &function.statements,
                    &local.name,
                )
        }) || !matches!(
            self.output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                ..
            ]
        ) {
            return;
        }
        self.move_instruction_before(5, 3);
        self.move_instruction_before(6, 5);
    }
}

fn unused_array_call_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(25).position(|window| {
        matches!(
            window,
            [
                Instruction::StoreWord { s: object, a: 1, .. },
                Instruction::LoadWord {
                    d: initialized_object,
                    a: 3,
                    ..
                },
                Instruction::StoreWord { s: entry, a: 1, .. },
                Instruction::Or {
                    a: copied_entry,
                    s: 3,
                    b: 3,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord { s: 0, a: first_store, .. },
                Instruction::StoreWord { s: 0, a: second_store, .. },
                Instruction::StoreWord { s: 0, a: third_store, .. },
                Instruction::StoreWord { s: 0, a: fourth_store, .. },
                Instruction::Or {
                    a: 3,
                    s: first_call_receiver,
                    b: first_call_receiver_again,
                },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::LoadFloatSingle { d: 1, .. },
                Instruction::LoadFloatSingle { d: 2, .. },
                Instruction::FloatMove { d: 3, b: 1 },
                Instruction::AddImmediate { d: 6, a: 0, .. },
                Instruction::BranchAndLink { .. },
                Instruction::Or {
                    a: 3,
                    s: second_call_receiver,
                    b: second_call_receiver_again,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediateShifted { d: first_high, a: 0, .. },
                Instruction::AddImmediate { d: 0, a: first_low_base, .. },
                Instruction::StoreWord { s: 0, a: first_callback_base, .. },
                Instruction::AddImmediateShifted { d: second_high, a: 0, .. },
                Instruction::AddImmediate { d: 0, a: second_low_base, .. },
                Instruction::StoreWord { s: 0, a: second_callback_base, .. },
            ] if *object == *initialized_object
                && *entry == *copied_entry
                && *object == *first_store
                && *object == *second_store
                && *object == *third_store
                && *object == *fourth_store
                && *entry == *first_call_receiver
                && *entry == *first_call_receiver_again
                && *entry == *second_call_receiver
                && *entry == *second_call_receiver_again
                && *object == *first_callback_base
                && *object == *second_callback_base
                && *first_high == *first_low_base
                && *second_high == *second_low_base
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_short_unrelated_sequence() {
        assert_eq!(
            unused_array_call_entry(&[
                Instruction::StoreWord {
                    s: 32,
                    a: 1,
                    offset: 28
                },
                Instruction::LoadWord {
                    d: 32,
                    a: 3,
                    offset: 44
                },
            ]),
            None
        );
    }
}
