//! Function designators used as pointer values.

use super::*;

impl Generator {
    /// Materialize a bare function designator for a pointer store.
    ///
    /// Functions always use an absolute address pair. GC 1.x/2.x completes
    /// that pair in r0; GC 3/Wii retain the high-half register as the value.
    pub(crate) fn emit_function_address_store_value(&mut self, name: &str) -> u8 {
        self.emit_function_address_store_value_avoiding_result(name, false)
    }

    fn emit_function_address_store_value_avoiding_result(
        &mut self,
        name: &str,
        avoid_result: bool,
    ) -> u8 {
        // A call result retained in its ABI register may need to become a later
        // call argument without an intervening instruction. Keep an overlapping
        // address arm out of that physical register; otherwise the instruction
        // liveness stream cannot see the implicit later argument use.
        let result_register = Eabi::general_result().number;
        let high = if avoid_result {
            self.fresh_virtual_general_avoiding(vec![result_register])
        } else {
            self.fresh_virtual_general()
        };
        self.emit_address_high(high, name);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        let source = if self.behavior.function_address_store_style
            == FunctionAddressStoreStyle::DirectAddress
        {
            high
        } else {
            GENERAL_SCRATCH
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: source,
            a: high,
            immediate: 0,
        });
        source
    }

    /// Materialize `condition ? function : 0` for a pointer store under the
    /// measured GC 1.x/2.x scratch-value policy.
    pub(crate) fn try_emit_function_address_null_store_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
    ) -> Compilation<Option<u8>> {
        if self.behavior.function_address_store_style != FunctionAddressStoreStyle::ScratchValue
            || constant_value(when_false) != Some(0)
        {
            return Ok(None);
        }
        let Expression::Variable(function) = when_true else {
            return Ok(None);
        };
        if self.locations.contains_key(function)
            || self.frame_slots.contains_key(function)
            || self.globals.contains_key(function)
            || self.fixed_address_arrays.contains_key(function)
            || self.fixed_address_objects.contains_key(function)
            || self.known_locals.contains(function)
        {
            return Ok(None);
        }

        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let false_arm = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, false_arm);
        let condition_name = match condition {
            Expression::Variable(name) => Some(name.as_str()),
            _ => None,
        };
        let result_register = Eabi::general_result().number;
        let another_result_value_is_live = self.locations.iter().any(|(name, location)| {
            Some(name.as_str()) != condition_name
                && location.class == ValueClass::General
                && location.register == result_register
        });
        let source = self.emit_function_address_store_value_avoiding_result(
            function,
            another_result_value_is_live,
        );
        let join = self.fresh_label();
        self.emit_branch_to(join);
        self.bind_label(false_arm);
        self.load_integer_constant(source, 0);
        self.bind_label(join);
        Ok(Some(source))
    }
}
