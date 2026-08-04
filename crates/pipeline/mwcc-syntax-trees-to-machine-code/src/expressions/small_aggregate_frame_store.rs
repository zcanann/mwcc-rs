//! Copy a one-word-or-smaller aggregate element into an automatic aggregate.
//!
//! Small union/bitfield configuration records frequently survive as aggregate
//! values in the compact IR even though MWCC copies their complete object
//! representation with one `lbz`, `lhz`, or `lwz`.  Keep that representation
//! copy separate from scalar conversion and member-field lowering.

#[allow(unused_imports)]
use super::*;

struct SmallAggregateFrameStore<'a> {
    slot: FrameSlot,
    aggregate: &'a Expression,
    member_offset: u32,
    index: &'a Expression,
    size: u32,
}

fn classify<'a>(
    generator: &Generator,
    target: &Expression,
    value: &'a Expression,
) -> Option<SmallAggregateFrameStore<'a>> {
    let Expression::Variable(name) = target else {
        return None;
    };
    let slot = generator.frame_slots.get(name).copied()?;
    let Type::Struct { size, .. } = slot.value_type else {
        return None;
    };
    if slot.is_array || !matches!(size, 1 | 2 | 4) {
        return None;
    }
    let Expression::Index { base, index } = value else {
        return None;
    };
    let Expression::Member {
        base: aggregate,
        offset: member_offset,
        member_type: Type::Struct {
            size: source_size,
            ..
        },
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    (*source_size == size).then_some(SmallAggregateFrameStore {
        slot,
        aggregate,
        member_offset: *member_offset,
        index,
        size,
    })
}

fn representation(size: u32) -> Pointee {
    match size {
        1 => Pointee::UnsignedChar,
        2 => Pointee::UnsignedShort,
        4 => Pointee::UnsignedInt,
        _ => unreachable!("classification restricts small aggregate sizes"),
    }
}

impl Generator {
    pub(super) fn try_emit_small_aggregate_frame_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some(copy) = classify(self, target, value) else {
            return Ok(false);
        };
        let element = representation(copy.size);
        let aggregate = self.member_base_register(copy.aggregate)?;
        let source = GENERAL_SCRATCH;
        if let Some(index) = constant_value(copy.index) {
            let offset = index
                .checked_mul(i64::from(copy.size))
                .and_then(|offset| offset.checked_add(i64::from(copy.member_offset)))
                .and_then(|offset| i16::try_from(offset).ok())
                .ok_or_else(|| {
                    Diagnostic::error("small aggregate element offset is out of range")
                })?;
            self.output.instructions.push(displacement_load(
                element,
                source,
                aggregate,
                offset,
            )?);
        } else {
            let index = self.general_register_of_leaf(copy.index)?;
            let scaled = if copy.size == 1 {
                index
            } else {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: index,
                        shift: copy.size.trailing_zeros() as u8,
                    });
                GENERAL_SCRATCH
            };
            let address = self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
            self.output.instructions.push(Instruction::Add {
                d: address,
                a: aggregate,
                b: scaled,
            });
            let offset = i16::try_from(copy.member_offset).map_err(|_| {
                Diagnostic::error("small aggregate member offset is out of range")
            })?;
            self.output.instructions.push(displacement_load(
                element,
                source,
                address,
                offset,
            )?);
        }
        self.output.instructions.push(displacement_store(
            element,
            source,
            1,
            copy.slot.offset,
        )?);
        self.written_slots.insert(copy.slot.offset);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representation_uses_an_unsigned_whole_object_copy() {
        assert_eq!(representation(1), Pointee::UnsignedChar);
        assert_eq!(representation(2), Pointee::UnsignedShort);
        assert_eq!(representation(4), Pointee::UnsignedInt);
    }
}
