//! Integer constant selects followed by one common additive offset.

use super::*;

impl Generator {
    /// Preserve `(condition ? C1 : C2) +/- K` as a select followed by one
    /// shared `addi`. Build 163 uses a full diamond; mainline preloads the false
    /// arm and conditionally overwrites it. Mainline consecutive constants stay
    /// with the branchless selector instead.
    pub(crate) fn try_emit_constant_select_with_common_offset(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        offset: i64,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        let true_constant = constant_value(when_true);
        let false_constant = constant_value(when_false);
        let integer_condition = match condition {
            Expression::Binary {
                operator,
                left,
                right,
            } if is_comparison(*operator) => {
                !self.is_float_value(left) && !self.is_float_value(right)
            }
            _ => !self.is_float_value(condition),
        };
        if self.non_leaf
            || origin != ConditionalOrigin::Ternary
            || !integer_condition
            || !true_constant.is_some_and(|value| i16::try_from(value).is_ok())
            || !false_constant.is_some_and(|value| i16::try_from(value).is_ok())
            || offset == 0
            || !(i16::MIN as i64..=i16::MAX as i64).contains(&offset)
        {
            return Ok(false);
        }
        let (true_constant, false_constant) = (true_constant.unwrap(), false_constant.unwrap());
        if self.behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::Branchless
            && true_constant.abs_diff(false_constant) == 1
        {
            return Ok(false);
        }

        self.output.anonymous_label_bump += 3;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        match self.behavior.integer_select_style {
            mwcc_versions::IntegerSelectStyle::BranchPreserving => {
                let false_branch = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                self.place_select_value(when_true, destination)?;
                let join_branch = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
                let false_arm = self.output.instructions.len();
                self.patch_forward(false_branch, false_arm);
                self.place_select_value(when_false, destination)?;
                let join = self.output.instructions.len();
                if let Instruction::Branch { target } =
                    &mut self.output.instructions[join_branch]
                {
                    *target = join;
                }
            }
            mwcc_versions::IntegerSelectStyle::Branchless => {
                self.place_select_value(when_false, destination)?;
                let false_branch = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                self.place_select_value(when_true, destination)?;
                let join = self.output.instructions.len();
                self.patch_forward(false_branch, join);
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: destination,
            immediate: offset as i16,
        });
        Ok(true)
    }
}
