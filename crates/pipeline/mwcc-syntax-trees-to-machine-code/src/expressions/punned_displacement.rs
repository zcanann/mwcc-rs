//! Displacement addressing through casts applied after byte-pointer arithmetic.

use super::*;

impl Generator {
    /// Resolve `(T *)(bytes +/- constant)` without materializing the adjusted
    /// address. Because the cast follows the arithmetic, `constant` is a byte
    /// displacement regardless of `T`; mwcc folds it into the load/store.
    pub(crate) fn punned_displacement_address(
        &self,
        pointer: &Expression,
    ) -> Option<(Pointee, u8, i16)> {
        let Expression::Cast {
            target_type: Type::Pointer(pointee),
            operand,
        } = pointer
        else {
            return None;
        };
        let Expression::Binary {
            operator,
            left,
            right,
        } = operand.as_ref()
        else {
            return None;
        };

        let (base, displacement) = match operator {
            BinaryOperator::Add => {
                if let Some(constant) = constant_value(right) {
                    (left.as_ref(), constant)
                } else {
                    (right.as_ref(), constant_value(left)?)
                }
            }
            BinaryOperator::Subtract => (left.as_ref(), constant_value(right)?.checked_neg()?),
            _ => return None,
        };
        let Expression::Variable(name) = base else {
            return None;
        };
        if let Some(slot) = self.frame_slots.get(name).filter(|slot| slot.is_array) {
            let offset = i64::from(slot.offset).checked_add(displacement)?;
            return Some((*pointee, 1, i16::try_from(offset).ok()?));
        }
        let address = self.lookup_general(name)?;
        Some((*pointee, address, i16::try_from(displacement).ok()?))
    }
}
