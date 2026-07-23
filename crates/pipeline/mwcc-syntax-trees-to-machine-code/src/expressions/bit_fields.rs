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

    /// Emit a bit-field used directly as a condition. PowerPC rotate-and-mask
    /// extraction has a record form, so MWCC lets the extraction set CR0 and
    /// branches without materializing the field and comparing it to zero.
    pub(crate) fn evaluate_bit_field_condition(
        &mut self,
        extracted: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        self.evaluate_bit_field_read(extracted, destination)?;
        let Some(last) = self.output.instructions.last_mut() else {
            return Err(Diagnostic::error(
                "a bit-field condition emitted no extraction instructions",
            ));
        };
        *last = match *last {
            Instruction::RotateAndMask {
                a,
                s,
                shift,
                begin,
                end,
            } => Instruction::RotateAndMaskRecord {
                a,
                s,
                shift,
                begin,
                end,
            },
            Instruction::ClearLeftImmediate { a, s, clear } => {
                Instruction::ClearLeftImmediateRecord { a, s, clear }
            }
            Instruction::AndContiguousMask { a, s, begin, end } => {
                Instruction::AndMaskRecord { a, s, begin, end }
            }
            _ => {
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: destination,
                        immediate: 0,
                    });
                return Ok(());
            }
        };
        Ok(())
    }
}
