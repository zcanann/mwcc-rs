//! Integer binary expressions whose operands are source-level absolute-value selects.
//!
//! Legacy MWCC evaluates these diamonds in reverse operand order, retains each
//! selected value in an independent allocator-owned home, and only then emits
//! the binary operation.  The ordinary single-scratch expression walker cannot
//! represent those overlapping lifetimes.

use super::*;

impl Generator {
    pub(crate) fn try_emit_integer_abs_pair_binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Add
            || crate::float_abs_select::abs_select_value(left).is_none()
            || crate::float_abs_select::abs_select_value(right).is_none()
        {
            return Ok(false);
        }

        let right_result = self.fresh_virtual_general();
        self.evaluate_general(right, right_result)?;
        let left_result = self.fresh_virtual_general();
        self.evaluate_general(left, left_result)?;
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: left_result,
            b: right_result,
        });
        Ok(true)
    }
}
