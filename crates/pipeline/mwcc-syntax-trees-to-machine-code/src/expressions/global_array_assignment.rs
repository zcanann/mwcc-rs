//! Assignment-valued global array stores retain the converted value explicitly.
//!
//! Store scheduling can consume r0 for the scaled index or put a constant in a
//! base temporary. Neither register is an implicit result of that store. Keep
//! the value in an allocator-owned live range through address formation instead.

use super::*;

impl Generator {
    pub(super) fn try_emit_global_array_assignment(
        &mut self,
        target: &Expression,
        value: &Expression,
        destination: Option<u32>,
    ) -> Compilation<bool> {
        let Expression::Index { base, index } = target else {
            return Ok(false);
        };
        let Expression::Variable(name) = base.as_ref() else {
            return Ok(false);
        };
        let Some(total_size) = self.global_array_address_extent(name) else {
            return Ok(false);
        };
        let Some(pointee) = self.globals.get(name).copied().and_then(pointee_of_type) else {
            return Ok(false);
        };
        if matches!(
            pointee,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
        ) {
            return Ok(false);
        }
        let source = self.fresh_virtual_general();
        self.with_reserved_inputs(target, |generator| {
            generator.emit_cast_to_integer(pointee.element(), value, source)
        })?;
        let address = self.fresh_virtual_general();
        let reserved = self.reserved.insert(source);
        let emitted = self.emit_global_array_element_address(name, total_size, index, address);
        if reserved {
            self.reserved.remove(&source);
        }
        emitted?;
        self.output
            .instructions
            .push(displacement_store(pointee, source, address, 0)?);
        if let Some(destination) = destination {
            self.output
                .instructions
                .push(Instruction::move_register(destination, source));
        }
        Ok(true)
    }
}
