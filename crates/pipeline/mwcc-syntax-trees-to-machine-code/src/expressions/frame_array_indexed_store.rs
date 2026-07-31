//! Variable-index stores into scalar arrays resident in the current frame.

use super::*;

fn element_scale(element: Pointee) -> Option<u8> {
    let size = element.size();
    size.is_power_of_two()
        .then_some(size.trailing_zeros() as u8)
}

impl Generator {
    /// Emit `frame_array[index] = value` through the indexed store family.
    /// Constant indices remain displacement stores in the caller.
    pub(crate) fn try_emit_variable_frame_array_store(
        &mut self,
        frame_offset: i16,
        element: Pointee,
        index: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if constant_value(index).is_some() {
            return Ok(false);
        }
        let Ok((index_register, width, _)) = self.leaf_info(index) else {
            return Ok(false);
        };
        if width < 32 {
            return Ok(false);
        }
        let Some(shift) = element_scale(element) else {
            return Ok(false);
        };

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
        let source = self.place_store_value(value, element)?;
        let base = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: 1,
            immediate: frame_offset,
        });
        self.output
            .instructions
            .push(indexed_store(element, source, base, scaled)?);
        self.written_slots.insert(frame_offset);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::element_scale;
    use mwcc_syntax_trees::Pointee;

    #[test]
    fn derives_index_shifts_from_scalar_element_widths() {
        assert_eq!(element_scale(Pointee::UnsignedChar), Some(0));
        assert_eq!(element_scale(Pointee::UnsignedShort), Some(1));
        assert_eq!(element_scale(Pointee::Int), Some(2));
        assert_eq!(element_scale(Pointee::Double), Some(3));
    }
}
