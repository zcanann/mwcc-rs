//! Build-163 leaf guards whose constant arm joins the final return.
//!
//! Selection initially uses the compact `load constant; bXXlr` form. When the
//! fallthrough arm has a nontrivial value schedule, build 163 instead keeps the
//! source diamond and sends the constant arm to the final shared `blr`.

use super::*;

fn shared_constant_guard(
    instructions: &[Instruction],
) -> Option<(usize, Instruction, u8, u8, usize)> {
    if instructions.len() < 6
        || !matches!(instructions.last(), Some(Instruction::BranchToLinkRegister))
    {
        return None;
    }
    let mut conditionals = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::BranchConditionalToLinkRegister {
                options,
                condition_bit,
            } => Some((index, *options, *condition_bit)),
            _ => None,
        });
    let (conditional, options, condition_bit) = conditionals.next()?;
    if conditionals.next().is_some() || conditional == 0 || conditional + 2 >= instructions.len() {
        return None;
    }
    let result_index = conditional - 1;
    let result = instructions[result_index].clone();
    if !matches!(result, Instruction::AddImmediateShifted { d: 3, a: 0, .. })
        || instructions[conditional + 1..instructions.len() - 1]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchToLinkRegister))
    {
        return None;
    }
    Some((
        result_index,
        result,
        options,
        condition_bit,
        instructions.len() - 1,
    ))
}

impl Generator {
    pub(crate) fn share_leaf_constant_guard_epilogue(&mut self) {
        if self.non_leaf
            || self.frame_size != 0
            || self.behavior.integer_select_style
                != mwcc_versions::IntegerSelectStyle::BranchPreserving
        {
            return;
        }
        let Some((result_index, result, options, condition_bit, final_return)) =
            shared_constant_guard(&self.output.instructions)
        else {
            return;
        };
        self.output.instructions[result_index] = Instruction::BranchConditionalForward {
            options: options ^ 8,
            condition_bit,
            target: result_index + 2,
        };
        self.output.instructions[result_index + 1] = result;
        crate::insert_instruction_retargeting(
            self,
            result_index + 2,
            Instruction::Branch {
                target: final_return,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_high_constant_before_a_conditional_return() {
        let instructions = vec![
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::load_immediate_shifted(3, -32768),
            Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            },
            Instruction::load_immediate_shifted(3, -13312),
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 36,
            },
            Instruction::BranchToLinkRegister,
        ];

        assert!(shared_constant_guard(&instructions).is_some());
    }
}
