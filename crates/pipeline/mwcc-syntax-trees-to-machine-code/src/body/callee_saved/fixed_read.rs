//! Leaf reads from fixed-address register banks.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts};
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A narrowed masked status read:
    /// `return (u16)(bank[k] & MASK);`.
    ///
    /// The mask itself already proves the result is representable as the
    /// declared `u16`, so mwcc writes the `rlwinm` directly into r3 rather than
    /// emitting a second `clrlwi` truncation after an r0 result.
    pub(crate) fn try_fixed_address_masked_narrow_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::UnsignedShort
        {
            return Ok(false);
        }
        let Some(return_expression) = &function.return_expression else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = peel_casts(return_expression)
        else {
            return Ok(false);
        };
        let (access, mask) = if let Some(mask) = constant_value(right) {
            (left.as_ref(), mask)
        } else {
            (right.as_ref(), constant_value(left).unwrap_or(0))
        };
        let Some((begin, end)) = rlwinm_mask(mask) else {
            return Ok(false);
        };
        // The direct-to-r3 form is valid only when the mask itself narrows the
        // value to 16 bits. Otherwise the return conversion remains observable.
        if mask as u32 & 0xffff_0000 != 0 {
            return Ok(false);
        }
        let Some((bank, index)) = fixed_slot(access) else {
            return Ok(false);
        };
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort {
            return Ok(false);
        }
        let (high, low) = crate::expressions::split_address(base_address);
        let offset = i16::try_from(low as i64 + index * 2)
            .map_err(|_| Diagnostic::error("fixed-address status displacement is out of range"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, high));
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                0,
                3,
                offset,
            )?);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin,
            end,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
