//! Entry scheduling for an object-state transaction with an unused scratch array.
//!
//! Build 163 keeps one integer zero and one floating one live across the initial
//! member stores, then publishes three callback addresses in an overlapped
//! high/low/store sequence.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_unused_array_state_entry(&mut self, function: &Function) {
        if self.inline_statement_body_substitutions != 0
            || !function.locals.iter().any(|local| {
                local.array_length.is_some()
                    && !super::structured_locals::body_uses_local(&function.statements, &local.name)
            })
        {
            return;
        }
        let Some(start) = unused_array_state_entry(&self.output.instructions) else {
            return;
        };
        if !schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start + 11,
            start + 17,
        ) {
            return;
        }

        // Save object; save entry; copy entry; then initialize object.
        self.move_instruction_before(start + 2, start + 1);
        self.move_instruction_before(start + 3, start + 2);

        // The pooled one used by the float member store remains live in f2 for
        // the first call. The integer zero from the preceding store run remains
        // live for the converted halfword store.
        match &mut self.output.instructions[start + 11] {
            Instruction::LoadFloatSingle { d, .. } => *d = 2,
            _ => unreachable!("state entry float load changed after recognition"),
        }
        match &mut self.output.instructions[start + 12] {
            Instruction::StoreFloatSingle { s, .. } => *s = 2,
            _ => unreachable!("state entry float store changed after recognition"),
        }
        for relative in [17, 13, 9] {
            self.remove_structured_condition_instruction(start + relative);
        }

        // Overlap each following callback's high half with publication of the
        // preceding address.
        self.move_instruction_before(start + 23, start + 22);
        self.move_instruction_before(start + 26, start + 25);

        // The folded assignment conversion still occupies one optimizer
        // ordinal in build 163 even though it has no runtime instructions.
        self.output.anonymous_label_bump += 1;
    }
}

fn unused_array_state_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(32).position(|window| {
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
                    b: 3
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreWord { s: 0, a: first_zero, .. },
                Instruction::StoreWord { s: 0, a: second_zero, .. },
                Instruction::StoreWord { s: 0, a: third_zero, .. },
                Instruction::StoreWord { s: 0, a: fourth_zero, .. },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: narrow_base,
                    ..
                },
                Instruction::LoadFloatSingle { d: 0, .. },
                Instruction::StoreFloatSingle {
                    s: 0,
                    a: float_base,
                    ..
                },
                Instruction::Or {
                    a: 3,
                    s: first_receiver,
                    b: first_receiver_again
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
                    s: second_receiver,
                    b: second_receiver_again
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediateShifted {
                    d: first_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: first_low_base,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: first_callback_base,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: second_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: second_low_base,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: second_callback_base,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: third_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: third_low_base,
                    ..
                },
                Instruction::StoreWord {
                    s: 0,
                    a: third_callback_base,
                    ..
                },
            ] if *object == *initialized_object
                && *entry == *copied_entry
                && [*first_zero, *second_zero, *third_zero, *fourth_zero, *narrow_base, *float_base]
                    .into_iter()
                    .all(|base| base == *object)
                && *entry == *first_receiver
                && *entry == *first_receiver_again
                && *entry == *second_receiver
                && *entry == *second_receiver_again
                && [*first_callback_base, *second_callback_base, *third_callback_base]
                    .into_iter()
                    .all(|base| base == *object)
                && *first_high == *first_low_base
                && *second_high == *second_low_base
                && *third_high == *third_low_base
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_short_unrelated_sequence() {
        assert_eq!(
            unused_array_state_entry(&[
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
