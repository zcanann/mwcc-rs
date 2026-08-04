//! Member loads through elements of an inline pointer array.
//!
//! The compact IR retains the pointed-to aggregate stride on
//! `owner->items[index]->field`, but the inline array itself contains four-byte
//! pointers.  This owner keeps those two layouts distinct: index the member by
//! pointer width, load the selected pointer, then access the aggregate member.

#[allow(unused_imports)]
use super::*;

struct MemberPointerArrayLoad<'a> {
    aggregate: &'a Expression,
    array_offset: u32,
    index: &'a Expression,
    member_offset: u32,
    element: Pointee,
}

fn classify<'a>(
    base: &'a Expression,
    member_offset: u32,
    member_type: Type,
    index_stride: Option<u32>,
) -> Option<MemberPointerArrayLoad<'a>> {
    index_stride.filter(|stride| *stride != 0)?;
    let Expression::Index {
        base: pointer_array,
        index,
    } = base
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: aggregate,
        offset: array_offset,
        element: Pointee::Pointer,
        index_stride: None,
    } = pointer_array.as_ref()
    else {
        return None;
    };
    Some(MemberPointerArrayLoad {
        aggregate,
        array_offset: *array_offset,
        index,
        member_offset,
        element: pointee_of_type(member_type)?,
    })
}

impl Generator {
    pub(super) fn try_emit_member_pointer_array_member_load(
        &mut self,
        base: &Expression,
        member_offset: u32,
        member_type: Type,
        index_stride: Option<u32>,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(load) = classify(base, member_offset, member_type, index_stride) else {
            return Ok(false);
        };

        let index = self.materialize_index_operand(load.index)?;
        let scaled = self.fresh_virtual_general_preferring(index);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: index,
                shift: 2,
            });
        let aggregate = self.member_base_register(load.aggregate)?;
        let pointer = self.fresh_virtual_general_preferring(destination);
        self.output.instructions.push(Instruction::Add {
            d: pointer,
            a: aggregate,
            b: scaled,
        });
        let array_displacement = self.emit_member_base_adjustment(pointer, load.array_offset);
        self.output.instructions.push(Instruction::LoadWord {
            d: pointer,
            a: pointer,
            offset: array_displacement,
        });
        let member_displacement =
            self.emit_member_base_adjustment(pointer, load.member_offset);
        self.output.instructions.push(displacement_load(
            load.element,
            destination,
            pointer,
            member_displacement,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_inline_pointer_array_before_the_pointed_to_member() {
        let base = Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("owner".into())),
                offset: 56,
                element: Pointee::Pointer,
                index_stride: None,
            }),
            index: Box::new(Expression::Variable("index".into())),
        };

        let load = classify(&base, 3, Type::UnsignedChar, Some(24))
            .expect("an inline pointer-array member load should classify");
        assert_eq!(load.array_offset, 56);
        assert_eq!(load.member_offset, 3);
        assert_eq!(load.element, Pointee::UnsignedChar);
    }

    #[test]
    fn rejects_an_inline_scalar_array() {
        let base = Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("owner".into())),
                offset: 56,
                element: Pointee::UnsignedInt,
                index_stride: None,
            }),
            index: Box::new(Expression::Variable("index".into())),
        };

        assert!(classify(&base, 0, Type::UnsignedChar, Some(24)).is_none());
    }
}
