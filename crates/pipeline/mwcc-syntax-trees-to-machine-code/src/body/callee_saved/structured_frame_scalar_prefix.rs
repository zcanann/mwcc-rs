//! Narrow address-taken scalars below linkage-first automatic arrays.
//!
//! Build 163 packs these slots by their physical width, in reverse local
//! declaration order, before placing the first array. Keeping this layout in a
//! small planner prevents the structured frame owner from assuming every
//! address-taken integer consumes a four-byte table lane.

#[allow(unused_imports)]
use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StructuredFrameScalarSlot {
    pub(super) offset: i16,
    pub(super) size: u8,
}

pub(super) struct StructuredFrameScalarPrefix {
    slots: HashMap<String, StructuredFrameScalarSlot>,
    end_offset: i16,
}

impl StructuredFrameScalarPrefix {
    pub(super) fn plan(
        parameters: &[&mwcc_syntax_trees::Parameter],
        locals: &[&LocalDeclaration],
    ) -> Option<Self> {
        let mut slots = HashMap::new();
        let mut offset = 8i16;
        for parameter in parameters {
            offset = align(offset, 4)?;
            slots.insert(
                parameter.name.clone(),
                StructuredFrameScalarSlot { offset, size: 4 },
            );
            offset = offset.checked_add(4)?;
        }
        for local in locals.iter().rev() {
            let size = physical_size(local.declared_type)?;
            offset = align(offset, size)?;
            slots.insert(
                local.name.clone(),
                StructuredFrameScalarSlot { offset, size },
            );
            offset = offset.checked_add(i16::from(size))?;
        }
        Some(Self {
            slots,
            end_offset: offset,
        })
    }

    pub(super) fn slot(&self, name: &str) -> Option<StructuredFrameScalarSlot> {
        self.slots.get(name).copied()
    }

    pub(super) fn end_offset(&self) -> i16 {
        self.end_offset
    }
}

fn physical_size(value_type: Type) -> Option<u8> {
    match value_type {
        Type::Char | Type::UnsignedChar => Some(1),
        Type::Short | Type::UnsignedShort => Some(2),
        Type::Int
        | Type::UnsignedInt
        | Type::Float
        | Type::Pointer(_)
        | Type::StructPointer { .. } => Some(4),
        _ => None,
    }
}

fn align(offset: i16, alignment: u8) -> Option<i16> {
    let alignment = i16::from(alignment);
    offset
        .checked_add(alignment - 1)
        .map(|offset| offset / alignment * alignment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn packs_trk_read_scalars_before_the_array() {
        let locals = [
            local("length", Type::UnsignedInt),
            local("start", Type::UnsignedInt),
            local("message_length", Type::UnsignedShort),
            local("options", Type::UnsignedChar),
            local("command", Type::UnsignedChar),
        ];
        let refs = locals.iter().collect::<Vec<_>>();

        let plan = StructuredFrameScalarPrefix::plan(&[], &refs).expect("scalar prefix");

        assert_eq!(plan.slot("command").unwrap().offset, 8);
        assert_eq!(plan.slot("options").unwrap().offset, 9);
        assert_eq!(plan.slot("message_length").unwrap().offset, 10);
        assert_eq!(plan.slot("start").unwrap().offset, 12);
        assert_eq!(plan.slot("length").unwrap().offset, 16);
        assert_eq!(plan.end_offset(), 20);
    }
}
