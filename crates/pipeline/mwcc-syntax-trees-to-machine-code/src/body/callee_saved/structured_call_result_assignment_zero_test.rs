//! Direct call results stored through a member and immediately tested for zero.
//!
//! Selection normally copies r3 into the condition scratch before publishing
//! an assignment result.  When both successors immediately redefine that
//! scratch, MWCC instead stores and compares r3 directly.  Recognize that
//! physical lifetime only after allocation and remove the dead copy while
//! preserving every instruction-index owner.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn fold_structured_call_result_assignment_zero_tests(&mut self) {
        while let Some(copy) =
            find_call_result_assignment_zero_test(&self.output.instructions)
        {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[copy + 1]
            else {
                unreachable!("the call-result member store was matched")
            };
            *s = Eabi::FIRST_GENERAL_ARGUMENT;
            let Instruction::CompareWordImmediate { a, .. } =
                &mut self.output.instructions[copy + 2]
            else {
                unreachable!("the call-result zero comparison was matched")
            };
            *a = Eabi::FIRST_GENERAL_ARGUMENT;
            crate::remove_instruction_retargeting_to_next(self, copy);
        }
    }
}

fn find_call_result_assignment_zero_test(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(5).enumerate().find_map(|(call, window)| {
        let [
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate {
                d: 0,
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                immediate: 0,
            },
            Instruction::StoreWord { s: 0, .. },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { target, .. },
        ] = window
        else {
            return None;
        };
        let branch = call + 4;
        scratch_is_discarded_at(instructions, branch + 1)
            .then(|| scratch_is_discarded_at(instructions, *target))
            .filter(|discarded| *discarded)
            .map(|_| call + 1)
    })
}

fn scratch_is_discarded_at(instructions: &[Instruction], successor: usize) -> bool {
    let Some(instruction) = instructions.get(successor) else {
        return successor == instructions.len();
    };
    let operands = mwcc_vreg::register_operands(instruction);
    let reads = operands.iter().any(|operand| {
        operand.class == mwcc_vreg::Class::General
            && operand.role == mwcc_vreg::RegisterRole::Use
            && operand.register == 0
    });
    let defines = operands.iter().any(|operand| {
        operand.class == mwcc_vreg::Class::General
            && operand.role == mwcc_vreg::RegisterRole::Define
            && operand.register == 0
    });
    defines && !reads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment_test(taken: Instruction) -> Vec<Instruction> {
        vec![
            Instruction::BranchAndLink { target: "decode".into() },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 31, offset: 172 },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 6,
            },
            Instruction::LoadWord { d: 0, a: 13, offset: 0 },
            taken,
        ]
    }

    #[test]
    fn recognizes_a_dead_condition_scratch_on_both_successors() {
        let instructions = assignment_test(Instruction::LoadWord {
            d: 0,
            a: 29,
            offset: 4,
        });

        assert_eq!(find_call_result_assignment_zero_test(&instructions), Some(1));
    }

    #[test]
    fn preserves_a_condition_scratch_read_by_the_taken_successor() {
        let instructions = assignment_test(Instruction::StoreWord {
            s: 0,
            a: 29,
            offset: 4,
        });

        assert_eq!(find_call_result_assignment_zero_test(&instructions), None);
    }
}
