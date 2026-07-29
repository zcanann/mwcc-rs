//! Scheduling for two independently guarded item-scaling calls.
//!
//! A retained receiver can own two nullable item pointers. The first call
//! scales directly from two receiver members; the second derives a clamped
//! scale from a shared table and an integer-to-float conversion. Build 163
//! retains each tested pointer in r3, keeps one shared-table base live, and
//! overlaps the conversion image with the clamp loads.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

const SECOND_SCHEDULE: [usize; 25] = [
    0, 1, 2, 3, 6, 11, 24, 4, 5, 7, 8, 12, 9, 14, 10, 18, 15, 20, 21, 22, 25, 27, 26, 28, 29,
];
const REMOVED_SECOND_INSTRUCTIONS: [usize; 5] = [13, 16, 17, 19, 23];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScaledItemTransaction {
    first: usize,
    second: usize,
    receiver: u8,
}

impl Generator {
    pub(crate) fn schedule_guarded_scaled_item_calls(&mut self, function: &Function) {
        if !has_unused_sixteen_byte_array(function) || self.frame_size != 32 {
            return;
        }
        let Some(transaction) = scaled_item_transaction(&self.output.instructions) else {
            return;
        };
        if !self.has_compact_scaled_item_frame(transaction.receiver) {
            return;
        }

        self.expand_scaled_item_frame(transaction.receiver);
        self.reuse_first_scaled_item_pointer(transaction.first);

        // Removing the first guarded reload shifts the second region once.
        let second = transaction.second - 1;
        self.rewrite_second_scaled_item_registers(second);
        for relative in REMOVED_SECOND_INSTRUCTIONS.into_iter().rev() {
            self.remove_structured_condition_instruction(second + relative);
        }
        self.apply_scaled_item_permutation(second);
        self.retarget_scaled_item_clamp(second);
    }

    fn reuse_first_scaled_item_pointer(&mut self, start: usize) {
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start] else {
            unreachable!("the first guarded pointer load was recognized")
        };
        *d = Eabi::general_result().number;
        let Instruction::CompareLogicalWordImmediate { a, .. } =
            &mut self.output.instructions[start + 1]
        else {
            unreachable!("the first guarded pointer comparison was recognized")
        };
        *a = Eabi::general_result().number;
        self.remove_structured_condition_instruction(start + 5);
    }

    fn rewrite_second_scaled_item_registers(&mut self, start: usize) {
        let window = &mut self.output.instructions[start..start + 30];
        match &mut window[0] {
            Instruction::LoadWord { d, .. } => *d = 3,
            _ => unreachable!("the second guarded pointer load was recognized"),
        }
        match &mut window[1] {
            Instruction::CompareLogicalWordImmediate { a, .. } => *a = 3,
            _ => unreachable!("the second guarded pointer comparison was recognized"),
        }
        match &mut window[3] {
            Instruction::LoadWord { d, .. } => *d = 4,
            _ => unreachable!("the converted integer load was recognized"),
        }
        match &mut window[4] {
            Instruction::XorImmediateShifted { a, s, .. } => {
                *a = 4;
                *s = 4;
            }
            _ => unreachable!("the signed conversion xor was recognized"),
        }
        match &mut window[5] {
            Instruction::StoreWord { s, offset, .. } => {
                *s = 4;
                *offset = 36;
            }
            _ => unreachable!("the conversion low-word store was recognized"),
        }
        match &mut window[7] {
            Instruction::LoadFloatDouble { d, .. } => *d = 2,
            _ => unreachable!("the signed conversion bias load was recognized"),
        }
        match &mut window[8] {
            Instruction::StoreWord { offset, .. } => *offset = 32,
            _ => unreachable!("the conversion high-word store was recognized"),
        }
        match &mut window[9] {
            Instruction::LoadFloatDouble { d, offset, .. } => {
                *d = 1;
                *offset = 32;
            }
            _ => unreachable!("the conversion image load was recognized"),
        }
        window[10] = Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 };
        match &mut window[11] {
            Instruction::LoadWord { d, .. } => *d = 5,
            _ => unreachable!("the shared table load was recognized"),
        }
        match &mut window[12] {
            Instruction::LoadFloatSingle { d, a, .. } => {
                *d = 3;
                *a = 5;
            }
            _ => unreachable!("the multiplier load was recognized"),
        }
        match &mut window[14] {
            Instruction::LoadFloatSingle { d, a, .. } => {
                *d = 0;
                *a = 5;
            }
            _ => unreachable!("the additive load was recognized"),
        }
        window[15] = Instruction::FloatMultiplyAddSingle {
            d: 2,
            a: 3,
            c: 1,
            b: 0,
        };
        match &mut window[18] {
            Instruction::LoadFloatSingle { d, a, .. } => {
                *d = 4;
                *a = 5;
            }
            _ => unreachable!("the clamp load was recognized"),
        }
        window[20] = Instruction::FloatCompareOrdered { a: 2, b: 4 };
        match &mut window[24] {
            Instruction::AddImmediate { d, .. } => *d = 6,
            _ => unreachable!("the attribute address was recognized"),
        }
        match &mut window[25] {
            Instruction::LoadFloatSingle { a, .. } => *a = 6,
            _ => unreachable!("the attribute load was recognized"),
        }
        match &mut window[26] {
            Instruction::FloatMultiplySingle { d, .. } => *d = 0,
            _ => unreachable!("the first final product was recognized"),
        }
        match &mut window[27] {
            Instruction::LoadFloatSingle { d, .. } => *d = 1,
            _ => unreachable!("the receiver-scale load was recognized"),
        }
    }

    fn apply_scaled_item_permutation(&mut self, start: usize) {
        let mut current: Vec<_> = (0..30)
            .filter(|index| !REMOVED_SECOND_INSTRUCTIONS.contains(index))
            .collect();
        for (destination, &original) in SECOND_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|candidate| *candidate == original)
                .expect("the scaled-item schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }

    fn retarget_scaled_item_clamp(&mut self, start: usize) {
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[start + 18]
        else {
            unreachable!("the scheduled clamp branch was recognized")
        };
        *target = start + 20;
    }

    fn has_compact_scaled_item_frame(&self, receiver: u8) -> bool {
        self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32,
                }
            )
        }) && self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s,
                    a: 1,
                    offset: 28,
                } if *s == receiver
            )
        }) && self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::LoadWord {
                    d,
                    a: 1,
                    offset: 28,
                } if *d == receiver
            )
        })
    }

    fn expand_scaled_item_frame(&mut self, receiver: u8) {
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32,
                } => {
                    *instruction = Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -48,
                    }
                }
                Instruction::StoreWord {
                    s,
                    a: 1,
                    offset: 28,
                } if *s == receiver => {
                    *instruction = Instruction::StoreWord {
                        s: receiver,
                        a: 1,
                        offset: 44,
                    }
                }
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 36,
                } => {
                    *instruction = Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: 52,
                    }
                }
                Instruction::LoadWord {
                    d,
                    a: 1,
                    offset: 28,
                } if *d == receiver => {
                    *instruction = Instruction::LoadWord {
                        d: receiver,
                        a: 1,
                        offset: 44,
                    }
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 32,
                } => {
                    *instruction = Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 48,
                    }
                }
                _ => {}
            }
        }
        self.frame_size = 48;
        self.int_to_float_scratch_next += 24;
        self.int_to_float_scratch_end += 24;
    }
}

fn has_unused_sixteen_byte_array(function: &Function) -> bool {
    function.locals.iter().any(|local| {
        !local.is_static
            && local.array_length == Some(16)
            && !body_uses_local(&function.statements, &local.name)
    })
}

fn scaled_item_transaction(instructions: &[Instruction]) -> Option<ScaledItemTransaction> {
    for first in 0..instructions.len().saturating_sub(38) {
        let Some(receiver) = first_scaled_item_call(instructions, first) else {
            continue;
        };
        let second = first + 8;
        if second_scaled_item_call(instructions, second, receiver) {
            return Some(ScaledItemTransaction {
                first,
                second,
                receiver,
            });
        }
    }
    None
}

fn first_scaled_item_call(instructions: &[Instruction], start: usize) -> Option<u8> {
    let [Instruction::LoadWord {
        d: tested,
        a: entry_base,
        offset: pointer_offset,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target,
    }, Instruction::LoadFloatSingle {
        d: 1,
        a: first_base,
        ..
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: second_base,
        ..
    }, Instruction::LoadWord {
        d: 3,
        a: reload_base,
        offset: reload_offset,
    }, Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 }, Instruction::BranchAndLink { .. }] =
        instructions.get(start..start + 8)?
    else {
        return None;
    };
    (*tested == *compared
        && *tested == 0
        && *entry_base == Eabi::general_result().number
        && (14..=31).contains(reload_base)
        && *reload_base == *first_base
        && *reload_base == *second_base
        && *pointer_offset == *reload_offset
        && *target == start + 8)
        .then_some(*reload_base)
}

fn second_scaled_item_call(instructions: &[Instruction], start: usize, receiver: u8) -> bool {
    matches!(
        instructions.get(start..start + 30),
        Some([
            Instruction::LoadWord { d: 0, a: pointer_base, offset: pointer_offset },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: outer_target,
            },
            Instruction::LoadWord { d: 3, a: integer_base, .. },
            Instruction::XorImmediateShifted {
                a: 0,
                s: 3,
                immediate: 0x8000,
            },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 0x4330,
            },
            Instruction::LoadFloatDouble { d: 3, a: 0, offset: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractSingle { d: 2, a: 0, b: 3 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 4, a: 3, .. },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 1, a: 3, .. },
            Instruction::FloatMultiplyAddSingle { d: 1, a: 2, c: 4, b: 1 },
            Instruction::FloatMove { d: 2, b: 1 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, .. },
            Instruction::FloatMove { d: 4, b: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: clamp_target,
            },
            Instruction::FloatMove { d: 2, b: 4 },
            Instruction::LoadWord { d: 3, a: reload_base, offset: reload_offset },
            Instruction::AddImmediate { d: 4, a: address_base, .. },
            Instruction::LoadFloatSingle { d: 0, a: 4, .. },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 0 },
            Instruction::LoadFloatSingle { d: 0, a: scale_base, .. },
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
            Instruction::BranchAndLink { .. },
        ]) if *pointer_base == receiver
            && *integer_base == receiver
            && *reload_base == receiver
            && *address_base == receiver
            && *scale_base == receiver
            && *pointer_offset == *reload_offset
            && *outer_target == start + 30
            && *clamp_target == start + 23
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_schedule_is_a_permutation_of_retained_instructions() {
        let retained: Vec<_> = (0..30)
            .filter(|index| !REMOVED_SECOND_INSTRUCTIONS.contains(index))
            .collect();
        let mut scheduled = SECOND_SCHEDULE.to_vec();
        scheduled.sort_unstable();
        assert_eq!(scheduled, retained);
    }
}
