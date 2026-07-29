//! Entry scheduling for a pointer returned by an inlined address accessor.
//!
//! When the original receiver is already saved for later arms, build 163 loads
//! its derived object directly into r3, prioritizes the guard load, and avoids
//! the generic allocator's later copy back into the first argument register.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_inlined_member_address_receiver(&mut self) {
        if self.inline_expansion_facts.body_value_substitutions == 0 {
            return;
        }
        let Some((start, working, payload)) =
            inlined_member_address_receiver(&self.output.instructions)
        else {
            return;
        };
        let first_payload_load = start + 2;
        let guard_load = start + 3;
        let argument_copy = start + 6;
        let receiver_float_load = start + 8;
        if self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if [first_payload_load, guard_load, argument_copy].contains(target)
            )
        }) {
            return;
        }

        match &mut self.output.instructions[start + 1] {
            Instruction::LoadWord { d, .. } => *d = Eabi::FIRST_GENERAL_ARGUMENT,
            _ => unreachable!("inlined address receiver was recognized"),
        }
        for index in [first_payload_load, guard_load] {
            match &mut self.output.instructions[index] {
                Instruction::LoadWord { a, .. } => *a = Eabi::FIRST_GENERAL_ARGUMENT,
                _ => unreachable!("inlined address receiver was recognized"),
            }
        }
        match &mut self.output.instructions[receiver_float_load] {
            Instruction::LoadFloatSingle { a, .. } => *a = Eabi::FIRST_GENERAL_ARGUMENT,
            _ => unreachable!("inlined address receiver was recognized"),
        }
        self.output
            .instructions
            .swap(first_payload_load, guard_load);
        if payload >= mwcc_vreg::VIRTUAL_BASE {
            // The erased accessor value still occupies the first volatile
            // allocator lane in MWCC's value graph. Preserve that hole so the
            // longer-lived payload pointer selects r5 after the receiver itself
            // is promoted directly into r3.
            self.register_avoid.insert(
                mwcc_vreg::VirtualRegister::new(
                    u32::from(payload - mwcc_vreg::VIRTUAL_BASE),
                    mwcc_vreg::Class::General,
                ),
                vec![Eabi::FIRST_GENERAL_ARGUMENT + 1],
            );
        }
        self.remove_structured_condition_instruction(argument_copy);

        debug_assert_ne!(working, Eabi::FIRST_GENERAL_ARGUMENT);
        debug_assert_ne!(payload, 0);
    }
}

fn inlined_member_address_receiver(instructions: &[Instruction]) -> Option<(usize, u8, u8)> {
    instructions.windows(11).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::Or {
                    a: saved,
                    s: 3,
                    b: 3,
                },
                Instruction::LoadWord {
                    d: working,
                    a: 3,
                    ..
                },
                Instruction::LoadWord {
                    d: payload,
                    a: first_base,
                    ..
                },
                Instruction::LoadWord {
                    d: 0,
                    a: guard_base,
                    ..
                },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward { target, .. },
                Instruction::Or {
                    a: 3,
                    s: copied,
                    b: copied_again,
                },
                Instruction::LoadFloatSingle {
                    a: payload_base, ..
                },
                Instruction::LoadFloatSingle {
                    a: receiver_base, ..
                },
                Instruction::FloatMultiplySingle { .. },
                Instruction::BranchAndLink { .. },
            ] if *saved >= 14
                && *working != 3
                && *working == *first_base
                && *working == *guard_base
                && *working == *copied
                && *working == *copied_again
                && *working == *receiver_base
                && *payload != 0
                && *payload == *payload_base
                && *target > start + 10 =>
            {
                Some((start, *working, *payload))
            }
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_saved_entry_and_inlined_receiver_prefix() {
        let instructions = vec![
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 5,
                a: 4,
                offset: 724,
            },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 8712,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 13,
            },
            Instruction::Or { a: 3, s: 4, b: 4 },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 5,
                offset: 136,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 4,
                offset: 296,
            },
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
            Instruction::BranchAndLink {
                target: String::new(),
            },
        ];
        assert_eq!(
            inlined_member_address_receiver(&instructions),
            Some((0, 4, 5))
        );
    }
}
