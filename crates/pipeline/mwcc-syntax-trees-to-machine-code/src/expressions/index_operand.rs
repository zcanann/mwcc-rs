//! General-register placement for array index expressions.

use super::*;

impl Generator {
    /// Return a register containing an array index. Plain local variables keep
    /// their existing homes; members, nested subscripts, calls, and other
    /// computed indices are evaluated into an allocator-owned virtual register.
    ///
    /// Keeping this policy outside individual array families lets a nested
    /// expression flow through byte arrays, struct arrays, and pointer arrays
    /// without teaching each address scheduler how that expression is formed.
    pub(crate) fn materialize_index_operand(
        &mut self,
        expression: &Expression,
    ) -> Compilation<u8> {
        if let Ok(register) = self.general_register_of_leaf(expression) {
            return Ok(register);
        }
        let register = self.fresh_virtual_general();
        self.evaluate_general(expression, register)?;
        Ok(register)
    }
}
