//! Scalar accesses to members of automatic aggregates.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Resolve an automatic aggregate member to its direct `r1` displacement.
    /// Keeping this separate from pointer-member addressing prevents a frame
    /// aggregate's source name from being mistaken for a pointer register.
    pub(crate) fn frame_subobject_slot(&self, target: &Expression) -> Option<(Pointee, i16)> {
        fn member_slot(
            generator: &Generator,
            base: &Expression,
            offset: u32,
            value_type: Type,
        ) -> Option<(Pointee, i16)> {
            let name = match base {
                Expression::Variable(name) => name,
                Expression::AddressOf { operand } => {
                    let Expression::Variable(name) = operand.as_ref() else {
                        return None;
                    };
                    name
                }
                _ => return None,
            };
            let slot = generator.frame_slots.get(name)?;
            if !matches!(slot.value_type, Type::Struct { .. }) || slot.is_array {
                return None;
            }
            let pointee = pointee_of_type(value_type)?;
            let offset = i16::try_from(offset).ok()?;
            Some((pointee, slot.offset.checked_add(offset)?))
        }

        match target {
            Expression::Member {
                base,
                offset,
                member_type,
                index_stride: None,
            } => member_slot(self, base, *offset, *member_type),
            Expression::Index { base, index } => {
                let Expression::Member {
                    base,
                    offset,
                    member_type,
                    index_stride: None,
                } = base.as_ref()
                else {
                    return None;
                };
                let index = constant_value(index)?;
                let pointee = pointee_of_type(*member_type)?;
                let offset = index
                    .checked_mul(i64::from(pointee.size()))
                    .and_then(|bytes| i64::from(*offset).checked_add(bytes))
                    .and_then(|bytes| u32::try_from(bytes).ok())?;
                member_slot(self, base, offset, *member_type)
            }
            _ => None,
        }
    }

    /// Store a scalar into a member (or constant-indexed member array) of an
    /// automatic aggregate.
    pub(crate) fn try_emit_frame_subobject_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((pointee, offset)) = self.frame_subobject_slot(target) else {
            return Ok(false);
        };
        let source = self.place_store_value(value, pointee)?;
        self.output
            .instructions
            .push(displacement_store(pointee, source, 1, offset)?);
        self.written_slots.insert(offset);
        Ok(true)
    }
}
