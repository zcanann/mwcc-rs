//! Floating-delta lifetimes for pairwise object-collision loops.
//!
//! The horizontal collision delta survives several conditional updates in f3,
//! while the vertical delta is retained in f0 between its truth and sign tests.
//! Reconstruct those final physical lifetimes after allocation so the scheduler
//! and the duplicated-expression cleanup agree on one value image.

#[allow(unused_imports)]
use super::*;

struct HorizontalDelta {
    start: usize,
    position_call: String,
    stack_offset: i16,
    receiver_offset: i16,
}

impl Generator {
    pub(crate) fn finalize_structured_object_collision_loop_delta(&mut self) {
        if !self.structured_object_collision_loop_entry {
            return;
        }
        let Some(delta) = horizontal_delta(&self.output.instructions) else {
            return;
        };
        if repeated_vertical_delta(&self.output.instructions).is_none() {
            return;
        }
        let start = delta.start;

        crate::insert_instruction_retargeting(
            self,
            start + 14,
            Instruction::FloatMove { d: 3, b: 0 },
        );
        let exit = match self.output.instructions[start + 23] {
            Instruction::BranchConditionalForward { target, .. } => target,
            _ => unreachable!("the horizontal overlap exit was matched"),
        };
        let zero_delta = match self.output.instructions[start + 25] {
            Instruction::BranchConditionalForward { target, .. } => target,
            _ => unreachable!("the horizontal zero-delta edge was matched"),
        };
        self.output.instructions[start..start + 27].clone_from_slice(&[
            Instruction::AddImmediate {
                d: 3,
                a: 24,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: delta.stack_offset,
            },
            Instruction::BranchAndLink {
                target: delta.position_call,
            },
            Instruction::LoadFloatSingle {
                d: 4,
                a: 31,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 24,
                immediate: delta.receiver_offset,
            },
            Instruction::LoadFloatSingle {
                d: 3,
                a: 30,
                offset: 44,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 27,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 2,
                a: 24,
                offset: 44,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 24,
                offset: delta.receiver_offset,
            },
            Instruction::FloatMultiplyAddSingle {
                d: 3,
                a: 4,
                c: 3,
                b: 0,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 1,
                offset: delta.stack_offset,
            },
            Instruction::FloatMultiplyAddSingle {
                d: 0,
                a: 2,
                c: 1,
                b: 0,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 3, b: 0 },
            Instruction::FloatCompareOrdered { a: 0, b: 31 },
            Instruction::FloatMove { d: 3, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: start + 18,
            },
            Instruction::FloatNegate { d: 2, b: 3 },
            Instruction::Branch { target: start + 19 },
            Instruction::FloatMove { d: 2, b: 3 },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 31,
                offset: 4,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 3,
                offset: 4,
            },
            Instruction::FloatAddSingle { d: 0, a: 1, b: 0 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: exit,
            },
            Instruction::FloatCompareUnordered { a: 3, b: 31 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: zero_delta,
            },
            Instruction::FloatCompareOrdered { a: 3, b: 31 },
        ]);

        let Some(vertical) = repeated_vertical_delta(&self.output.instructions) else {
            return;
        };
        let Instruction::FloatSubtractSingle { d, .. } =
            &mut self.output.instructions[vertical + 2]
        else {
            unreachable!("the vertical subtraction was matched")
        };
        *d = 0;
        let Instruction::FloatCompareUnordered { a, .. } =
            &mut self.output.instructions[vertical + 3]
        else {
            unreachable!("the vertical truth test was matched")
        };
        *a = 0;
        for _ in 0..3 {
            crate::remove_instruction_retargeting_to_next(self, vertical + 5);
        }
        let Instruction::FloatCompareOrdered { a, .. } =
            &mut self.output.instructions[vertical + 5]
        else {
            unreachable!("the vertical sign test was matched")
        };
        *a = 0;

        if let Some(later) = self.output.instructions[vertical + 6..]
            .windows(3)
            .position(|window| {
                matches!(
                    window,
                    [
                        Instruction::FloatCompareUnordered { a: 2, b: 31 },
                        Instruction::BranchConditionalForward { .. },
                        Instruction::FloatCompareOrdered { a: 2, b: 31 },
                    ]
                )
            })
            .map(|relative| vertical + 6 + relative)
        {
            let Instruction::FloatCompareUnordered { a, .. } = &mut self.output.instructions[later]
            else {
                unreachable!()
            };
            *a = 3;
            let Instruction::FloatCompareOrdered { a, .. } =
                &mut self.output.instructions[later + 2]
            else {
                unreachable!()
            };
            *a = 3;
        }
    }
}

fn horizontal_delta(instructions: &[Instruction]) -> Option<HorizontalDelta> {
    instructions
        .windows(26)
        .enumerate()
        .find_map(|(start, window)| {
            let [
                Instruction::Or { a: 3, s: 24, b: 24 },
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: stack_offset,
                },
                Instruction::BranchAndLink {
                    target: position_call,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 24,
                    immediate: receiver_offset,
                },
                Instruction::LoadFloatSingle { d: 4, a: 31, offset: 0 },
                Instruction::LoadFloatSingle { d: 3, a: 30, offset: 44 },
                Instruction::LoadFloatSingle { d: 0, a: 27, offset: 0 },
                Instruction::FloatMultiplyAddSingle {
                    d: 3,
                    a: 4,
                    c: 3,
                    b: 0,
                },
                Instruction::LoadFloatSingle { d: 2, a: 24, offset: 44 },
                Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 1,
                    offset: second_stack_offset,
                },
                Instruction::FloatMultiplyAddSingle {
                    d: 0,
                    a: 2,
                    c: 1,
                    b: 0,
                },
                Instruction::FloatSubtractSingle { d: 2, a: 3, b: 0 },
                Instruction::FloatCompareOrdered { a: 2, b: 31 },
                Instruction::BranchConditionalForward { .. },
                Instruction::FloatNegate { d: 1, b: 2 },
                Instruction::Branch { .. },
                Instruction::FloatMove { d: 1, b: 2 },
                Instruction::LoadFloatSingle { d: 3, a: 31, offset: 4 },
                Instruction::LoadFloatSingle { d: 0, a: 3, offset: 4 },
                Instruction::FloatAddSingle { d: 0, a: 3, b: 0 },
                Instruction::FloatCompareOrdered { a: 1, b: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::FloatCompareUnordered { a: 2, b: 31 },
                Instruction::BranchConditionalForward { .. },
                Instruction::FloatCompareOrdered { a: 2, b: 31 },
            ] = window
            else {
                return None;
            };
            (stack_offset == second_stack_offset).then(|| HorizontalDelta {
                start,
                position_call: position_call.clone(),
                stack_offset: *stack_offset,
                receiver_offset: *receiver_offset,
            })
        })
}

fn repeated_vertical_delta(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(9).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 27,
                    offset: 8,
                },
                Instruction::LoadFloatSingle { d: 0, a: 1, .. },
                Instruction::FloatSubtractSingle { d: 1, a: 1, b: 0 },
                Instruction::FloatCompareUnordered { a: 1, b: 31 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 27,
                    offset: 8,
                },
                Instruction::LoadFloatSingle { d: 0, a: 1, .. },
                Instruction::FloatSubtractSingle { d: 1, a: 1, b: 0 },
                Instruction::FloatCompareOrdered { a: 1, b: 31 },
            ]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_recomputed_vertical_delta() {
        let mut instructions = Vec::new();
        for _ in 0..2 {
            instructions.extend([
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 27,
                    offset: 8,
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 1,
                    offset: 44,
                },
                Instruction::FloatSubtractSingle { d: 1, a: 1, b: 0 },
                if instructions.is_empty() {
                    Instruction::FloatCompareUnordered { a: 1, b: 31 }
                } else {
                    Instruction::FloatCompareOrdered { a: 1, b: 31 }
                },
            ]);
            if instructions.len() == 4 {
                instructions.push(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 20,
                });
            }
        }

        assert_eq!(repeated_vertical_delta(&instructions), Some(0));
    }
}
