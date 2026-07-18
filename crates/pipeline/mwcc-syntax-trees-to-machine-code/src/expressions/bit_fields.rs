//! Source-level bit-field extraction policy.
//!
//! The parser retains bit-field provenance around the ordinary unit-load /
//! shift / mask tree. That lets the generation profile change only the unit
//! load's register without changing identical explicit C expressions.

use super::*;

impl Generator {
    pub(crate) fn evaluate_bit_field_read(
        &mut self,
        extracted: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        if self.behavior.bit_field_load_placement == BitFieldLoadPlacement::Scratch {
            return self.evaluate_general(extracted, destination);
        }

        if let Expression::Binary {
            operator,
            left,
            right,
        } = extracted
        {
            if matches!(
                operator,
                BinaryOperator::BitAnd | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
            ) && self.try_emit_rotate_mask_loading_value_into(
                *operator,
                left,
                right,
                destination,
                Some(destination),
            )? {
                return Ok(());
            }
            if *operator == BinaryOperator::BitAnd
                && self.try_emit_mask_loading_value_into(left, right, destination, destination)?
            {
                return Ok(());
            }
        }

        // A full-unit field has no extraction operator. Any future bit-field
        // shape that does not fuse also retains correct semantics here.
        self.evaluate_general(extracted, destination)
    }
}
