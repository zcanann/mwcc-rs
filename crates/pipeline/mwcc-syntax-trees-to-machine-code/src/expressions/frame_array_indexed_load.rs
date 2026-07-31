//! Variable-index loads from scalar arrays resident in the current frame.

use super::*;

impl Generator {
    /// Emit `frame_array[index]` through the indexed load family. A frame array
    /// has no register-valued pointer location: materialize `r1 + slot.offset`
    /// for this access instead of passing the scratch sentinel to `lfsx/lwzx`.
    pub(crate) fn try_emit_variable_frame_array_load(
        &mut self,
        name: &str,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if constant_value(index).is_some() {
            return Ok(false);
        }
        let Some(slot) = self
            .frame_slots
            .get(name)
            .copied()
            .filter(|slot| slot.is_array)
        else {
            return Ok(false);
        };
        let Some(element) = pointee_of_type(slot.value_type) else {
            return Ok(false);
        };
        let Ok((index_register, width, _)) = self.leaf_info(index) else {
            return Ok(false);
        };
        if width < 32 || !element.size().is_power_of_two() {
            return Ok(false);
        }

        let shift = element.size().trailing_zeros() as u8;
        let scaled = if shift == 0 {
            index_register
        } else {
            let scaled = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index_register,
                    shift,
                });
            scaled
        };
        let base = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: 1,
            immediate: slot.offset,
        });
        self.output
            .instructions
            .push(indexed_load(element, destination, base, scaled)?);
        Ok(true)
    }
}
