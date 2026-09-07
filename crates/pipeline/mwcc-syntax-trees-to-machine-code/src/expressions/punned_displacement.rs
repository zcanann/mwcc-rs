//! Displacement addressing through casts applied after pointer arithmetic.

use super::*;

impl Generator {
    /// Fold `(T *)(base +/- constant)` into an access displacement. The
    /// arithmetic base supplies the stride; the outer cast supplies the access
    /// width. These types need not agree, as in `(short *)((int *)p + 3)`.
    pub(crate) fn punned_displacement_address(
        &mut self,
        pointer: &Expression,
    ) -> Compilation<Option<(Pointee, u8, i16)>> {
        let Some((pointee, base, displacement)) = cast_displacement(pointer) else {
            return Ok(None);
        };
        let Some(name) = casted_pointer_base_name(base) else {
            return Ok(None);
        };
        let slot = self.frame_slots.get(name).copied();
        let stride = match base {
            Expression::Cast { target_type, .. } => pointer_stride(*target_type),
            _ => {
                if let Some(slot) = slot {
                    if slot.is_array {
                        self.frame_row_bytes
                            .get(name)
                            .copied()
                            .map(u32::from)
                            .or_else(|| {
                                pointee_of_type(slot.value_type).map(|p| u32::from(p.size()))
                            })
                    } else {
                        pointer_stride(slot.value_type)
                    }
                } else if let Some(location) = self.locations.get(name) {
                    location
                        .stride
                        .or_else(|| location.pointee.map(|p| u32::from(p.size())))
                } else {
                    self.globals.get(name).and_then(|ty| pointer_stride(*ty))
                }
            }
        };
        let Some(displacement) = stride
            .and_then(|stride| displacement.checked_mul(i64::from(stride)))
            .and_then(|offset| i16::try_from(offset).ok())
        else {
            return Ok(None);
        };
        if let Some(slot) = slot.filter(|slot| slot.is_array) {
            let Some(offset) = slot.offset.checked_add(displacement) else {
                return Ok(None);
            };
            return Ok(Some((pointee, 1, offset)));
        }
        // A frame-backed pointer's register may be stale. Materialize its value
        // just like a global pointer, instead of treating its slot as the target.
        if slot.is_none() {
            if let Some(address) = self.lookup_general(name) {
                return Ok(Some((pointee, address, displacement)));
            }
        }
        let address = self.fresh_virtual_general_preferring(3);
        self.evaluate_general(base, address)?;
        Ok(Some((pointee, address, displacement)))
    }
}

fn pointer_stride(ty: Type) -> Option<u32> {
    match ty {
        Type::Pointer(pointee) => Some(u32::from(pointee.size())),
        Type::StructPointer { element_size } if element_size != 0 => Some(element_size),
        _ => None,
    }
}

fn cast_displacement(pointer: &Expression) -> Option<(Pointee, &Expression, i64)> {
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
    Some((*pointee, base, displacement))
}

fn casted_pointer_base_name(mut expression: &Expression) -> Option<&str> {
    while let Expression::Cast {
        target_type: Type::Pointer(_) | Type::StructPointer { .. },
        operand,
    } = expression
    {
        expression = operand;
    }
    let Expression::Variable(name) = expression else {
        return None;
    };
    Some(name)
}
