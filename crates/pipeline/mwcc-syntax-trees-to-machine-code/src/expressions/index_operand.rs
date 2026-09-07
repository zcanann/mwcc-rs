//! General-register placement for array index expressions.

use super::*;

impl Generator {
    /// Terminal address schedules overwrite the index. A surrounding statement
    /// or named virtual may still own it, so hand that schedule a private copy.
    pub(crate) fn preserve_address_index(&mut self, index: u8) -> u8 {
        let named_virtual = mwcc_vreg::Reg::is_virtual_field(index)
            && self.locations.values().any(|location| {
                location.class == ValueClass::General && location.register == index
            });
        if self.reserved.contains(&index) || named_virtual {
            let temporary = self.fresh_virtual_general();
            self.output
                .instructions
                .push(Instruction::move_register(temporary, index));
            temporary
        } else {
            index
        }
    }

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
