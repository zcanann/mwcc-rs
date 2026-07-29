//! Scheduling retained inline state beside an unused source scratch array.
//!
//! The dead array keeps its frame bytes while a tail mutating inline retains a
//! value-graph lane. Build 163 batches the three saved-home stores, then fills
//! a guarded float-load slot with the already-saved receiver.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_unused_array_mutating_inline(&mut self, function: &Function) {
        if self.inline_statement_body_substitutions == 0
            || self.legacy_inline_expansion_frame_bytes == 0
            || !function.locals.iter().any(|local| {
                local.array_length.is_some()
                    && !super::structured_locals::body_uses_local(&function.statements, &local.name)
            })
        {
            return;
        }
        let Some((start, receiver, payload, entry)) =
            unused_array_saved_home_prefix(&self.output.instructions)
        else {
            return;
        };
        let Some(float_copy) =
            guarded_float_receiver_copy(&self.output.instructions, receiver, payload)
        else {
            return;
        };

        // save receiver; save payload; save entry; copy entry; initialize receiver;
        // load guard; initialize payload.
        self.move_instruction_before(start + 2, start + 1);
        self.move_instruction_before(start + 4, start + 2);
        self.move_instruction_before(start + 5, start + 3);
        self.move_instruction_before(start + 6, start + 5);

        // The independent payload float load fills the receiver copy's issue slot.
        self.move_instruction_before(float_copy + 1, float_copy);
        self.output.anonymous_label_bump += 6;
        debug_assert_ne!(entry, receiver);
    }
}

fn unused_array_saved_home_prefix(instructions: &[Instruction]) -> Option<(usize, u8, u8, u8)> {
    instructions
        .windows(8)
        .enumerate()
        .find_map(|(start, window)| match window {
            [Instruction::StoreWord {
                s: receiver,
                a: 1,
                offset: receiver_slot,
            }, Instruction::LoadWord {
                d: initialized_receiver,
                a: 3,
                ..
            }, Instruction::StoreWord {
                s: payload,
                a: 1,
                offset: payload_slot,
            }, Instruction::LoadWord {
                d: initialized_payload,
                a: payload_base,
                ..
            }, Instruction::StoreWord {
                s: entry,
                a: 1,
                offset: entry_slot,
            }, Instruction::Or {
                a: copied_entry,
                s: 3,
                b: 3,
            }, Instruction::LoadWord {
                d: 0,
                a: guard_base,
                ..
            }, Instruction::CompareWordImmediate { a: 0, immediate: 0 }]
                if *receiver == *initialized_receiver
                    && *receiver == *payload_base
                    && *receiver == *guard_base
                    && *payload == *initialized_payload
                    && *entry == *copied_entry
                    && *receiver != *payload
                    && *payload != *entry
                    && *receiver_slot == *payload_slot + 4
                    && *payload_slot == *entry_slot + 4 =>
            {
                Some((start, *receiver, *payload, *entry))
            }
            _ => None,
        })
}

fn guarded_float_receiver_copy(
    instructions: &[Instruction],
    receiver: u8,
    payload: u8,
) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::Or {
                    a: 3,
                    s: copied,
                    b: copied_again,
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: payload_base,
                    ..
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: receiver_base,
                    ..
                },
                Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
                Instruction::BranchAndLink { .. },
            ] if *copied == receiver
                && *copied_again == receiver
                && *payload_base == payload
                && *receiver_base == receiver
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_interleaved_three_home_entry() {
        let instructions = vec![
            Instruction::StoreWord {
                s: 32,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 32,
                a: 3,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 33,
                a: 1,
                offset: 24,
            },
            Instruction::LoadWord {
                d: 33,
                a: 32,
                offset: 724,
            },
            Instruction::StoreWord {
                s: 34,
                a: 1,
                offset: 20,
            },
            Instruction::Or { a: 34, s: 3, b: 3 },
            Instruction::LoadWord {
                d: 0,
                a: 32,
                offset: 224,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ];
        assert_eq!(
            unused_array_saved_home_prefix(&instructions),
            Some((0, 32, 33, 34))
        );
    }
}
