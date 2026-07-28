//! Automatic typed-array planning for structured stack frames.
//!
//! Source arrays remain distinct frame slots even when they are unused.
//! Keeping their validation and byte accounting here lets the structured body
//! owner compose scalar, aggregate, and flattened multidimensional arrays with
//! the rest of the frame.

#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredFrameArrays<'a> {
    pub(super) arrays: Vec<&'a LocalDeclaration>,
    pub(super) total_bytes: i16,
}

pub(super) fn plan_structured_frame_arrays<'a>(
    locals: &'a [LocalDeclaration],
    statements: &[Statement],
) -> Option<StructuredFrameArrays<'a>> {
    let arrays: Vec<_> = locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .collect();
    let mut total_bytes = 0i16;
    for array in &arrays {
        if array.is_static || array.initializer.is_some() {
            return None;
        }
        let element_bytes = match array.declared_type {
            Type::Struct { size, .. } => size,
            value_type => u32::from(value_type.width() / 8),
        };
        let bytes = element_bytes
            .checked_mul(u32::from(array.array_length?))
            .filter(|bytes| *bytes != 0)?;
        if array
            .data_bytes
            .as_ref()
            .is_some_and(|image| image.len() > bytes as usize)
            || !array.data_relocations.is_empty()
        {
            return None;
        }
        total_bytes = total_bytes.checked_add(i16::try_from(bytes).ok()?)?;
    }
    Some(StructuredFrameArrays {
        arrays,
        total_bytes,
    })
}

/// Mainline MWCC groups a run of initialized automatic arrays by slot size and
/// allocates equal-sized images in reverse declaration order. Larger images
/// follow the smaller size class. This is observable when the pooled copy-in
/// image remains in declaration order: each source image is stored into its
/// independently assigned frame slot.
pub(super) fn initialized_array_placement_order<'a>(
    arrays: &[&'a LocalDeclaration],
) -> Vec<&'a LocalDeclaration> {
    let initialized_count = arrays
        .iter()
        .filter(|array| array.data_bytes.is_some())
        .count();
    if initialized_count < 2 {
        return arrays.to_vec();
    }

    let mut indexed: Vec<_> = arrays.iter().copied().enumerate().collect();
    indexed.sort_by_key(|(source_index, array)| {
        (
            array_byte_size(array).unwrap_or(u32::MAX),
            std::cmp::Reverse(*source_index),
        )
    });
    indexed.into_iter().map(|(_, array)| array).collect()
}

pub(super) fn array_byte_size(array: &LocalDeclaration) -> Option<u32> {
    let element_bytes = match array.declared_type {
        Type::Struct { size, .. } => size,
        value_type => u32::from(value_type.width() / 8),
    };
    element_bytes.checked_mul(u32::from(array.array_length?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn byte_array(name: &str, declared_type: Type, length: u16) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
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
    fn retains_multiple_source_byte_arrays_as_one_reserved_region() {
        let locals = vec![
            byte_array("prefix", Type::UnsignedChar, 4),
            byte_array("suffix", Type::Char, 20),
        ];

        let plan = plan_structured_frame_arrays(&locals, &[]).expect("valid byte arrays");

        assert_eq!(plan.arrays.len(), 2);
        assert_eq!(plan.total_bytes, 24);
    }

    #[test]
    fn retains_an_unused_non_byte_padding_array() {
        let locals = vec![byte_array("words", Type::UnsignedInt, 4)];

        let plan = plan_structured_frame_arrays(&locals, &[]).expect("unused padding");

        assert_eq!(plan.total_bytes, 16);
    }

    #[test]
    fn retains_a_used_typed_array_for_element_lowering() {
        let locals = vec![byte_array("words", Type::UnsignedInt, 4)];
        let statements = vec![Statement::Expression(Expression::Variable(
            "words".into(),
        ))];

        let plan = plan_structured_frame_arrays(&locals, &statements).expect("typed array");
        assert_eq!(plan.total_bytes, 16);
    }

    #[test]
    fn retains_an_aggregate_array_larger_than_one_byte_of_metadata() {
        let locals = vec![byte_array(
            "nodes",
            Type::Struct { size: 8, align: 4 },
            33,
        )];

        let plan = plan_structured_frame_arrays(&locals, &[]).expect("large node stack");
        assert_eq!(plan.total_bytes, 264);
    }

    #[test]
    fn initialized_equal_size_arrays_receive_reverse_slots_before_larger_images() {
        let mut date = byte_array("date", Type::UnsignedChar, 32);
        date.data_bytes = Some(vec![0]);
        let mut time = byte_array("time", Type::UnsignedChar, 32);
        time.data_bytes = Some(vec![0]);
        let mut ampm = byte_array("ampm", Type::UnsignedChar, 32);
        ampm.data_bytes = Some(vec![0]);
        let mut scratch = byte_array("scratch", Type::UnsignedChar, 256);
        scratch.data_bytes = Some(vec![0]);
        let arrays = [&date, &time, &ampm, &scratch];

        let ordered = initialized_array_placement_order(&arrays);

        assert_eq!(
            ordered
                .iter()
                .map(|array| array.name.as_str())
                .collect::<Vec<_>>(),
            ["ampm", "time", "date", "scratch"]
        );
    }

    #[test]
    fn a_lone_initialized_array_retains_source_placement() {
        let mut buffer = byte_array("buffer", Type::UnsignedChar, 32);
        buffer.data_bytes = Some(vec![0]);

        assert_eq!(
            initialized_array_placement_order(&[&buffer])[0].name,
            "buffer"
        );
    }
}
