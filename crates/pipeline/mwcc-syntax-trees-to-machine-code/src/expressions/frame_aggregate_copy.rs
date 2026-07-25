//! Aggregate copies whose destination lives in an automatic frame slot.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Copy an aggregate from a frame slot or typed struct-pointer source into
    /// a frame-resident aggregate lvalue. A single word scratch is enough;
    /// overlap chooses the memmove-safe direction.
    pub(crate) fn try_emit_frame_aggregate_copy(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if let Expression::Variable(target_name) = target {
            if self.try_emit_frame_aggregate_call_assignment(target_name, value)? {
                return Ok(true);
            }
        }
        let Some((target_offset, target_size)) = self.frame_aggregate_target(target)? else {
            return Ok(false);
        };
        let (source_register, source_offset, source_size, source_is_frame) = match value {
            Expression::Variable(source_name) => {
                let Some(source) = self.frame_slots.get(source_name).copied() else {
                    return Ok(false);
                };
                let Type::Struct { size, .. } = source.value_type else {
                    return Ok(false);
                };
                (1, source.offset, size, true)
            }
            Expression::Dereference { pointer } => {
                let Expression::Variable(source_name) = pointer.as_ref() else {
                    return Ok(false);
                };
                let Some(location) = self.locations.get(source_name) else {
                    return Ok(false);
                };
                let Some(size) = location.stride else {
                    return Ok(false);
                };
                if location.class != ValueClass::General {
                    return Ok(false);
                }
                (location.register, 0i16, size, false)
            }
            Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size, .. },
                index_stride: None,
            } => {
                let source_register = self.member_base_register(base)?;
                let source_offset = i16::try_from(*offset).map_err(|_| {
                    Diagnostic::error("frame aggregate member source is out of range")
                })?;
                (source_register, source_offset, *size, false)
            }
            _ => return Ok(false),
        };
        if source_size != target_size || source_size == 0 || source_size % 4 != 0 {
            return Err(Diagnostic::error(
                "a frame aggregate copy requires equal, word-sized objects (roadmap)",
            ));
        }

        let bytes = i16::try_from(source_size)
            .map_err(|_| Diagnostic::error("frame aggregate copy is too large"))?;
        let backwards = if source_is_frame {
            let source_end = source_offset
                .checked_add(bytes)
                .ok_or_else(|| Diagnostic::error("frame aggregate source is out of range"))?;
            target_offset > source_offset && target_offset < source_end
        } else {
            false
        };
        let words = source_size / 4;
        let indices: Box<dyn Iterator<Item = u32>> = if backwards {
            Box::new((0..words).rev())
        } else {
            Box::new(0..words)
        };
        for word in indices {
            let displacement = i16::try_from(word * 4)
                .map_err(|_| Diagnostic::error("frame aggregate word offset is out of range"))?;
            let source_word_offset = source_offset.checked_add(displacement).ok_or_else(|| {
                Diagnostic::error("frame aggregate source word is out of range")
            })?;
            let destination_offset = target_offset.checked_add(displacement).ok_or_else(|| {
                Diagnostic::error("frame aggregate destination word is out of range")
            })?;
            self.output.instructions.push(Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: source_register,
                offset: source_word_offset,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: GENERAL_SCRATCH,
                a: 1,
                offset: destination_offset,
            });
            self.written_slots.insert(destination_offset);
        }
        Ok(true)
    }

    fn frame_aggregate_target(
        &self,
        target: &Expression,
    ) -> Compilation<Option<(i16, u32)>> {
        match target {
            Expression::Variable(name) => {
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(None);
                };
                let Type::Struct { size, .. } = slot.value_type else {
                    return Ok(None);
                };
                Ok(Some((slot.offset, size)))
            }
            Expression::Dereference { pointer } => {
                let mut pointer = pointer.as_ref();
                while let Expression::Cast { operand, .. } = pointer {
                    pointer = operand;
                }
                let Expression::AddressOf { operand } = pointer else {
                    return Ok(None);
                };
                self.frame_aggregate_target(operand)
            }
            Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size, .. },
                index_stride: None,
            } => {
                let name = match base.as_ref() {
                    Expression::Variable(name) => name,
                    Expression::AddressOf { operand } => {
                        let Expression::Variable(name) = operand.as_ref() else {
                            return Ok(None);
                        };
                        name
                    }
                    _ => return Ok(None),
                };
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(None);
                };
                let Type::Struct {
                    size: container_size,
                    ..
                } = slot.value_type
                else {
                    return Ok(None);
                };
                if offset
                    .checked_add(*size)
                    .is_none_or(|end| end > container_size)
                {
                    return Err(Diagnostic::error(
                        "frame aggregate member lies outside its containing object",
                    ));
                }
                let target_offset =
                    crate::frame::checked_frame_member_offset(slot.offset, *offset)?;
                Ok(Some((target_offset, *size)))
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else {
                    return Ok(None);
                };
                let Some(slot) = self
                    .frame_slots
                    .get(name)
                    .copied()
                    .filter(|slot| slot.is_array)
                else {
                    return Ok(None);
                };
                let Type::Struct { size, .. } = slot.value_type else {
                    return Ok(None);
                };
                let Some(index) = constant_value(index) else {
                    return Ok(None);
                };
                let byte_offset = index
                    .checked_mul(i64::from(size))
                    .filter(|offset| *offset >= 0)
                    .and_then(|offset| i16::try_from(offset).ok())
                    .ok_or_else(|| {
                        Diagnostic::error("frame aggregate array index is out of range")
                    })?;
                let target_offset = slot.offset.checked_add(byte_offset).ok_or_else(|| {
                    Diagnostic::error("frame aggregate array element is out of range")
                })?;
                let element_end = i32::from(byte_offset) + i32::try_from(size).unwrap_or(i32::MAX);
                if element_end > i32::from(slot.size) {
                    return Err(Diagnostic::error(
                        "frame aggregate array element lies outside its slot",
                    ));
                }
                Ok(Some((target_offset, size)))
            }
            _ => Ok(None),
        }
    }
}
