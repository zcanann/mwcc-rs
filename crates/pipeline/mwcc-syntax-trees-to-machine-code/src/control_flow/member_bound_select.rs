//! Register-retaining MIN/MAX selection over two object members.
//!
//! MWCC keeps both comparison loads live: the false arm is the destination,
//! the true arm is the scratch operand, and the taken comparison overwrites the
//! destination with one move. Re-emitting either member after the branch loses
//! that common-subexpression schedule.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Type;

impl Generator {
    pub(crate) fn try_emit_member_bound_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        else {
            return Ok(false);
        };
        if !is_comparison(*operator)
            || !structurally_equal(left, when_true)
            || !structurally_equal(right, when_false)
        {
            return Ok(false);
        }
        let Some((_, _, true_type)) = as_member(when_true) else {
            return Ok(false);
        };
        let Some((_, _, false_type)) = as_member(when_false) else {
            return Ok(false);
        };
        if destination == GENERAL_SCRATCH
            || !matches!(true_type, Type::Int | Type::UnsignedInt)
            || !matches!(false_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }

        self.evaluate_general(when_false, destination)?;
        self.evaluate_general(when_true, GENERAL_SCRATCH)?;
        if self.usual_integer_binary_signedness(when_true, when_false)? {
            self.output.instructions.push(Instruction::CompareWord {
                a: GENERAL_SCRATCH,
                b: destination,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWord {
                    a: GENERAL_SCRATCH,
                    b: destination,
                });
        }
        let (options, condition_bit) =
            false_branch_bo_bi(*operator).expect("the operator is a comparison");
        let join = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::move_register(destination, GENERAL_SCRATCH));
        self.patch_forward(join, self.output.instructions.len());
        Ok(true)
    }
}
