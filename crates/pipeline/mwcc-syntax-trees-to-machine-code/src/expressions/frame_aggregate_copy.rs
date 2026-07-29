//! Aggregate copies whose destination lives in an automatic frame slot.

#[allow(unused_imports)]
use super::*;

enum FrameAggregateSource<'a> {
    Memory {
        register: u8,
        offset: i16,
        size: u32,
        is_frame: bool,
    },
    Global {
        name: &'a str,
        size: u32,
    },
}

impl FrameAggregateSource<'_> {
    fn size(&self) -> u32 {
        match self {
            Self::Memory { size, .. } | Self::Global { size, .. } => *size,
        }
    }
}

fn frame_aggregate_array_element(slot: FrameSlot, index: i64) -> Compilation<Option<(i16, u32)>> {
    if !slot.is_array {
        return Ok(None);
    }
    let Type::Struct { size, .. } = slot.value_type else {
        return Ok(None);
    };
    let byte_offset = index
        .checked_mul(i64::from(size))
        .filter(|offset| *offset >= 0)
        .and_then(|offset| i16::try_from(offset).ok())
        .ok_or_else(|| Diagnostic::error("frame aggregate array index is out of range"))?;
    let element_end = i32::from(byte_offset) + i32::try_from(size).unwrap_or(i32::MAX);
    if element_end > i32::try_from(slot.size).unwrap_or(i32::MAX) {
        return Err(Diagnostic::error(
            "frame aggregate array element lies outside its slot",
        ));
    }
    let offset = slot.offset.checked_add(byte_offset).ok_or_else(|| {
        Diagnostic::error("frame aggregate array element address is out of range")
    })?;
    Ok(Some((offset, size)))
}

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
        let source = match value {
            Expression::Variable(source_name) => {
                if let Some(source) = self.frame_slots.get(source_name).copied() {
                    let Type::Struct { size, .. } = source.value_type else {
                        return Ok(false);
                    };
                    FrameAggregateSource::Memory {
                        register: 1,
                        offset: source.offset,
                        size,
                        is_frame: true,
                    }
                } else {
                    let Some(Type::Struct { size, .. }) =
                        self.addressable_globals.get(source_name).copied()
                    else {
                        return Ok(false);
                    };
                    // Objects larger than the EABI small-data threshold have an
                    // ordinary absolute address. Smaller aggregates need an
                    // SDA/SDA2-specific multiword schedule, which remains a
                    // separate source form rather than silently misaddressing it.
                    if size <= 8 {
                        return Ok(false);
                    }
                    FrameAggregateSource::Global {
                        name: source_name,
                        size,
                    }
                }
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
                FrameAggregateSource::Memory {
                    register: location.register,
                    offset: 0,
                    size,
                    is_frame: false,
                }
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
                FrameAggregateSource::Memory {
                    register: source_register,
                    offset: source_offset,
                    size: *size,
                    is_frame: false,
                }
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else {
                    return Ok(false);
                };
                let Some(index) = constant_value(index) else {
                    return Ok(false);
                };
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(false);
                };
                let Some((source_offset, size)) = frame_aggregate_array_element(slot, index)?
                else {
                    return Ok(false);
                };
                FrameAggregateSource::Memory {
                    register: 1,
                    offset: source_offset,
                    size,
                    is_frame: true,
                }
            }
            _ => return Ok(false),
        };
        let source_size = source.size();
        if source_size != target_size || source_size == 0 || source_size % 4 != 0 {
            return Err(Diagnostic::error(
                "a frame aggregate copy requires equal, word-sized objects (roadmap)",
            ));
        }

        let (source_register, source_offset, source_is_frame) = match source {
            FrameAggregateSource::Global { name, size } => {
                return self.emit_global_to_frame_aggregate_copy(name, target_offset, size);
            }
            FrameAggregateSource::Memory {
                register,
                offset,
                is_frame,
                ..
            } => (register, offset, is_frame),
        };
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
            let source_word_offset = source_offset
                .checked_add(displacement)
                .ok_or_else(|| Diagnostic::error("frame aggregate source word is out of range"))?;
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

    /// Large file-scope aggregates use their absolute address even in a
    /// small-data build. MWCC pipelines the first two words before their stores,
    /// then reuses the second scratch for any remaining words.
    fn emit_global_to_frame_aggregate_copy(
        &mut self,
        name: &str,
        target_offset: i16,
        size: u32,
    ) -> Compilation<bool> {
        let address_high = self.fresh_virtual_general_preferring(3);
        let source_address = self.fresh_virtual_general_preferring(5);
        self.emit_address_high(address_high, name);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: source_address,
            a: address_high,
            immediate: 0,
        });

        let first_word = self.fresh_virtual_general_preferring(4);
        self.output.instructions.push(Instruction::LoadWord {
            d: first_word,
            a: source_address,
            offset: 0,
        });
        let second_word = (size >= 8).then(|| {
            let register = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: source_address,
                offset: 4,
            });
            register
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: first_word,
            a: 1,
            offset: target_offset,
        });
        self.written_slots.insert(target_offset);
        if let Some(second_word) = second_word {
            let destination_offset = target_offset.checked_add(4).ok_or_else(|| {
                Diagnostic::error("frame aggregate destination word is out of range")
            })?;
            self.output.instructions.push(Instruction::StoreWord {
                s: second_word,
                a: 1,
                offset: destination_offset,
            });
            self.written_slots.insert(destination_offset);
        }
        for displacement in (8..size).step_by(4) {
            let displacement = i16::try_from(displacement)
                .map_err(|_| Diagnostic::error("frame aggregate word offset is out of range"))?;
            let destination_offset = target_offset.checked_add(displacement).ok_or_else(|| {
                Diagnostic::error("frame aggregate destination word is out of range")
            })?;
            self.output.instructions.push(Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: source_address,
                offset: displacement,
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

    fn frame_aggregate_target(&self, target: &Expression) -> Compilation<Option<(i16, u32)>> {
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
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(None);
                };
                let Some(index) = constant_value(index) else {
                    return Ok(None);
                };
                frame_aggregate_array_element(slot, index)
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_constant_aggregate_frame_array_element() {
        let slot = FrameSlot {
            offset: 20,
            class: ValueClass::General,
            size: 36,
            value_type: Type::Struct { size: 12, align: 4 },
            parameter_register: None,
            is_array: true,
        };

        assert_eq!(
            frame_aggregate_array_element(slot, 2).unwrap(),
            Some((44, 12))
        );
        assert!(frame_aggregate_array_element(slot, 3).is_err());
    }
}
