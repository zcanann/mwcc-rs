//! Interleaved automatic-array and aggregate frame placement.
//!
//! Legacy MWCC lays unobserved automatic frame objects in reverse declaration
//! order. Planning arrays and aggregates as separate regions changes offsets
//! when an aggregate is declared between two padding arrays. This owner models
//! the measured byte-array/vector form without coupling source object order to
//! statement emission.

use super::structured_frame_arrays::{align_offset, array_byte_size, array_stack_alignment};
#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredInterleavedFrameLayout {
    offsets: std::collections::HashMap<String, i16>,
    local_region_bytes: i16,
    saved_area_gap_bytes: i16,
}

impl StructuredInterleavedFrameLayout {
    pub(super) fn plan(
        function: &Function,
        arrays: &[&LocalDeclaration],
        aggregates: &[&LocalDeclaration],
        arrays_are_unused: bool,
        frame_convention: FrameConvention,
    ) -> Option<Self> {
        let [first_array, second_array] = arrays else {
            return None;
        };
        let [aggregate] = aggregates else {
            return None;
        };
        let Type::Struct {
            size: aggregate_size,
            align: aggregate_alignment,
        } = aggregate.declared_type
        else {
            return None;
        };
        if !arrays_are_unused
            || frame_convention != FrameConvention::LinkageFirst
            || array_byte_size(first_array)? != 4
            || array_byte_size(second_array)? != 20
            || aggregate_size != 12
            || aggregate_alignment != 4
        {
            return None;
        }
        let frame_objects: Vec<_> = function
            .locals
            .iter()
            .filter_map(|local| {
                if arrays.iter().any(|array| array.name == local.name) {
                    Some(FrameObject::Array(local))
                } else if local.name == aggregate.name {
                    Some(FrameObject::Aggregate(local))
                } else {
                    None
                }
            })
            .collect();
        if !matches!(
            frame_objects.as_slice(),
            [
                FrameObject::Array(first),
                FrameObject::Aggregate(middle),
                FrameObject::Array(last),
            ] if first.name == first_array.name
                && middle.name == aggregate.name
                && last.name == second_array.name
        ) {
            return None;
        }

        let mut offsets = std::collections::HashMap::new();
        // Linkage-first frames retain one incoming-value lane below automatic
        // objects. This family starts at 16 rather than the generic local
        // region's logical base of 8.
        let frame_object_base = 16i16;
        let mut offset = frame_object_base;
        for object in frame_objects.into_iter().rev() {
            let (alignment, bytes, name) = match object {
                FrameObject::Array(array) => (
                    array_stack_alignment(array),
                    i16::try_from(array_byte_size(array)?).ok()?,
                    &array.name,
                ),
                FrameObject::Aggregate(aggregate) => {
                    let Type::Struct { size, align } = aggregate.declared_type else {
                        return None;
                    };
                    (
                        i16::from(align.max(1)),
                        i16::try_from(size).ok()?,
                        &aggregate.name,
                    )
                }
            };
            offset = align_offset(offset, alignment)?;
            offsets.insert(name.clone(), offset);
            offset = offset.checked_add(bytes)?;
        }
        Some(Self {
            offsets,
            local_region_bytes: offset.checked_sub(frame_object_base)?,
            // This frame family keeps the source-local table lane between the
            // final automatic object and the contiguous saved-register area.
            saved_area_gap_bytes: 8,
        })
    }

    pub(super) fn offset(&self, name: &str) -> Option<i16> {
        self.offsets.get(name).copied()
    }

    pub(super) fn local_region_bytes(&self) -> i16 {
        self.local_region_bytes
    }

    pub(super) fn saved_area_gap_bytes(&self) -> i16 {
        self.saved_area_gap_bytes
    }
}

enum FrameObject<'a> {
    Array(&'a LocalDeclaration),
    Aggregate(&'a LocalDeclaration),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array(name: &str, length: u16) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::UnsignedChar,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(length),
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn places_an_aggregate_between_reverse_declaration_order_arrays() {
        let function = Function {
            return_type: Type::Void,
            name: "frame".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                array("prefix", 4),
                LocalDeclaration {
                    declared_type: Type::Struct { size: 12, align: 4 },
                    name: "vector".into(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                },
                array("suffix", 20),
            ],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let arrays = [&function.locals[0], &function.locals[2]];
        let aggregates = [&function.locals[1]];

        let layout = StructuredInterleavedFrameLayout::plan(
            &function,
            &arrays,
            &aggregates,
            true,
            FrameConvention::LinkageFirst,
        )
        .unwrap();

        assert_eq!(layout.offset("suffix"), Some(16));
        assert_eq!(layout.offset("vector"), Some(36));
        assert_eq!(layout.offset("prefix"), Some(48));
        assert_eq!(layout.local_region_bytes(), 36);
        assert_eq!(layout.saved_area_gap_bytes(), 8);
    }
}
