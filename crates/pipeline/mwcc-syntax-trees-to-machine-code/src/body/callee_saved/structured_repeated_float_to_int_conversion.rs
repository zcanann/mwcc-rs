//! Reuse of a float-to-integer result across distinct conversion images.
//!
//! Build 163 can retain one `fctiwz` result in its scratch FPR while publishing
//! the same source conversion to two stack images. The images remain distinct
//! optimizer values, but repeating the conversion itself would lengthen the
//! scheduled body by one instruction.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn reuse_structured_float_to_int_result(&mut self) {
        let Some(repeated) = repeated_float_to_int_conversion(&self.output.instructions) else {
            return;
        };
        self.output.instructions.remove(repeated);
        self.labels.removed_retargeting_to_next(repeated, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != repeated);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > repeated {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > repeated =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }
}

fn repeated_float_to_int_conversion(instructions: &[Instruction]) -> Option<usize> {
    for (first, instruction) in instructions.iter().enumerate() {
        let Instruction::ConvertToIntegerWordZero {
            d: converted,
            b: source,
        } = *instruction
        else {
            continue;
        };
        let Some(Instruction::StoreFloatDouble {
            s: first_image,
            a: 1,
            offset: first_offset,
        }) = instructions.get(first + 1)
        else {
            continue;
        };
        let Some(Instruction::LoadWord {
            a: 1,
            offset: first_word,
            ..
        }) = instructions.get(first + 2)
        else {
            continue;
        };
        if *first_image != converted || *first_word != first_offset + 4 {
            continue;
        }

        let search_end = instructions.len().min(first + 12);
        for repeated in first + 3..search_end {
            if !matches!(
                instructions[repeated],
                Instruction::ConvertToIntegerWordZero { d, b }
                    if d == converted && b == source
            ) {
                continue;
            }
            let Some(Instruction::StoreFloatDouble {
                s: second_image,
                a: 1,
                offset: second_offset,
            }) = instructions.get(repeated + 1)
            else {
                continue;
            };
            let Some(Instruction::LoadWord {
                a: 1,
                offset: second_word,
                ..
            }) = instructions.get(repeated + 2)
            else {
                continue;
            };
            if *second_image != converted
                || *second_word != second_offset + 4
                || second_offset == first_offset
            {
                continue;
            }
            let invalidated = instructions[first + 1..repeated].iter().any(|between| {
                matches!(
                    between,
                    Instruction::BranchAndLink { .. }
                        | Instruction::Branch { .. }
                        | Instruction::BranchConditionalForward { .. }
                ) || mwcc_vreg::register_operands(between)
                    .into_iter()
                    .any(|operand| {
                        operand.class == mwcc_vreg::Class::Float
                            && operand.role == mwcc_vreg::RegisterRole::Define
                            && (operand.register == converted || operand.register == source)
                    })
            });
            if !invalidated {
                return Some(repeated);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeated_pair() -> Vec<Instruction> {
        vec![
            Instruction::ConvertToIntegerWordZero { d: 0, b: 4 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 40,
            },
            Instruction::Or { a: 30, s: 3, b: 3 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 4 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
        ]
    }

    #[test]
    fn recognizes_distinct_images_of_one_conversion_result() {
        assert_eq!(repeated_float_to_int_conversion(&repeated_pair()), Some(5));
    }

    #[test]
    fn rejects_a_result_clobbered_between_images() {
        let mut instructions = repeated_pair();
        instructions.insert(3, Instruction::FloatMove { d: 0, b: 1 });
        assert_eq!(repeated_float_to_int_conversion(&instructions), None);
    }
}
