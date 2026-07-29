//! Scheduling for a guarded signed-integer-derived float call argument.
//!
//! A retained integer multiplied by a local float constant is converted only
//! after a classifier call succeeds. MWCC overlaps the independent bias and
//! multiplier loads with the conversion image stores, then materializes the
//! guarded receiver immediately before the final call.

#[allow(unused_imports)]
use super::*;

const GUARDED_FLOAT_ARGUMENT_SCHEDULE: [usize; 12] = [1, 4, 2, 3, 0, 10, 5, 9, 6, 7, 8, 11];

impl Generator {
    pub(crate) fn schedule_guarded_float_argument(&mut self) {
        let Some(start) = guarded_float_argument(&self.output.instructions) else {
            return;
        };
        let saved_integer_argument = match self.output.instructions[start + 10] {
            Instruction::Or { s, b, .. } if s == b => s,
            Instruction::AddImmediate {
                a, immediate: 0, ..
            } => a,
            _ => unreachable!("the final integer argument copy was matched"),
        };

        let mut current: Vec<usize> = (0..GUARDED_FLOAT_ARGUMENT_SCHEDULE.len()).collect();
        for (destination, &original) in GUARDED_FLOAT_ARGUMENT_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("the guarded float schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }

        let window = &mut self.output.instructions[start..start + 12];
        let Instruction::LoadFloatSingle { d: multiplier, .. } = &mut window[4] else {
            unreachable!("the multiplier load was matched")
        };
        *multiplier = 0;
        let Instruction::LoadFloatDouble { d: image, .. } = &mut window[8] else {
            unreachable!("the conversion image load was matched")
        };
        *image = 1;
        window[9] = Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 };
        window[10] = Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 };

        if let Some(copy) = self.output.instructions[..start]
            .iter()
            .rposition(|instruction| {
                matches!(
                    instruction,
                    Instruction::Or { a, s: 4, b: 4 }
                        if *a == saved_integer_argument
                )
            })
        {
            self.output.instructions[copy] = Instruction::AddImmediate {
                d: saved_integer_argument,
                a: 4,
                immediate: 0,
            };
        }
    }
}

fn guarded_float_argument(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(12).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadFloatSingle { d: multiplier, a: 0, .. },
                Instruction::XorImmediateShifted {
                    a: 0,
                    s: converted,
                    immediate: 0x8000,
                },
                Instruction::StoreWord { s: 0, a: 1, offset: low },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 0x4330,
                },
                Instruction::LoadFloatDouble { d: bias, a: 0, .. },
                Instruction::StoreWord { s: 0, a: 1, offset: high },
                Instruction::LoadFloatDouble {
                    d: image,
                    a: 1,
                    offset: image_offset,
                },
                Instruction::FloatSubtractSingle {
                    d: difference,
                    a: image_source,
                    b: bias_source,
                },
                Instruction::FloatMultiplySingle {
                    d: product,
                    a: difference_source,
                    c: multiplier_source,
                },
                Instruction::LoadWord { d: 3, .. },
                argument_copy,
                Instruction::BranchAndLink { .. },
            ] if *low == high + 4
                && *high == *image_offset
                && *converted != 0
                && *multiplier == *multiplier_source
                && *bias == *bias_source
                && *image == *image_source
                && *image == *difference
                && *difference == *difference_source
                && *product == *multiplier
                && (matches!(argument_copy, Instruction::Or { a: 4, s, b } if s == b)
                    || matches!(
                        argument_copy,
                        Instruction::AddImmediate {
                            d: 4,
                            immediate: 0,
                            ..
                        }
                    ))
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_complete_guarded_conversion_call_window() {
        let instructions = [
            Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::XorImmediateShifted {
                a: 0,
                s: 31,
                immediate: 0x8000,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 0x4330,
            },
            Instruction::LoadFloatDouble {
                d: 2,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 24,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 24,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 },
            Instruction::FloatMultiplySingle { d: 1, a: 0, c: 1 },
            Instruction::LoadWord {
                d: 3,
                a: 29,
                offset: 6516,
            },
            Instruction::move_register(4, 30),
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert_eq!(guarded_float_argument(&instructions), Some(0));
    }
}
