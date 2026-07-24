//! Function designators used as pointer values.

use super::*;

impl Generator {
    /// Materialize a bare function designator for a pointer store.
    ///
    /// Functions always use an absolute address pair. GC 1.x/2.x completes
    /// that pair in r0; GC 3/Wii retain the high-half register as the value.
    pub(crate) fn emit_function_address_store_value(&mut self, name: &str) -> u8 {
        let high = self.fresh_virtual_general();
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
        let source = self.emit_function_address_store_value(function);
        let join = self.fresh_label();
        self.emit_branch_to(join);
        self.bind_label(false_arm);
        self.load_integer_constant(source, 0);
        self.bind_label(join);
        Ok(Some(source))
    }
}
