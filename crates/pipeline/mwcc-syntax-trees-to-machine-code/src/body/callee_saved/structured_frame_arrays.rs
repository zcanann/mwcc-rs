//! Automatic typed-array planning for structured stack frames.
//!
//! Source arrays ordinarily remain distinct frame slots even when they are
//! unused. Mainline optimization has one important exception: dead initialized
//! arrays lose their storage and copy work while retaining their source images
//! in `.rodata`, including holes inside a partially live group. Image retention
//! belongs to `automatic_rodata`; this owner plans only executable frame storage.

#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredFrameArrays<'a> {
    pub(super) arrays: Vec<&'a LocalDeclaration>,
    pub(super) image_sources: Vec<&'a LocalDeclaration>,
    pub(super) total_bytes: i16,
}

pub(super) fn plan_structured_frame_arrays<'a>(
    function: &'a Function,
) -> Option<StructuredFrameArrays<'a>> {
    let source_arrays: Vec<_> = function
        .locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .collect();
    for array in &source_arrays {
        if array.is_static || array.initializer.is_some() {
            return None;
        }
        // The structured frame computes its local-region extent before its
        // final array base is known. A widened declarator alignment therefore
        // needs base-aware padding, not merely a wider slot displacement; let
        // the generic frame owner handle simple bodies until that planner owns
        // the complete aligned region.
        if array.attribute_alignment.is_some_and(|requested| {
            i16::try_from(requested)
                .map(|requested| requested > array_stack_alignment(array))
                .unwrap_or(true)
        }) {
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
    }
    let arrays: Vec<_> = source_arrays
        .iter()
        .copied()
        .filter(|array| {
            array.data_bytes.is_none()
                || crate::analysis::function_uses_name(function, &array.name)
        })
        .collect();
    let mut total_bytes = 0i16;
    for array in structured_array_placement_order(&arrays) {
        total_bytes = align_offset(total_bytes, array_stack_alignment(array))?;
        total_bytes =
            total_bytes.checked_add(i16::try_from(array_byte_size(array)?).ok()?)?;
    }
    Some(StructuredFrameArrays {
        arrays,
        image_sources: source_arrays
            .into_iter()
            .filter(|array| array.data_bytes.is_some())
            .collect(),
        total_bytes,
    })
}

/// Mainline MWCC groups a run of initialized automatic arrays by slot size and
/// allocates equal-sized images in reverse declaration order. Larger images
/// follow the smaller size class. This is observable when the pooled copy-in
/// image remains in declaration order: each source image is stored into its
/// independently assigned frame slot.
pub(super) fn structured_array_placement_order<'a>(
    arrays: &[&'a LocalDeclaration],
) -> Vec<&'a LocalDeclaration> {
    let initialized_count = arrays
        .iter()
        .filter(|array| array.data_bytes.is_some())
        .count();
    if initialized_count < 2 {
        return arrays.iter().copied().rev().collect();
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

/// Match the general frame planner's automatic-array alignment.
pub(super) fn array_stack_alignment(array: &LocalDeclaration) -> i16 {
    match array.declared_type {
        Type::Struct { align, .. } => i16::from(align.max(1)),
        Type::Double => 8,
        _ => 4,
    }
}

pub(super) fn align_offset(offset: i16, alignment: i16) -> Option<i16> {
    debug_assert!(offset >= 0);
    debug_assert!(alignment > 0);
    offset
        .checked_add(alignment - 1)
        .map(|value| value / alignment * alignment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function_with_locals(locals: Vec<LocalDeclaration>) -> Function {
        Function {
            return_type: Type::Void,
            name: "arrays".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals,
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
        }
    }

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
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn retains_multiple_source_byte_arrays_as_one_reserved_region() {
        let locals = vec![
            byte_array("prefix", Type::UnsignedChar, 4),
            byte_array("suffix", Type::Char, 20),
        ];

        let function = function_with_locals(locals);
        let plan = plan_structured_frame_arrays(&function).expect("valid byte arrays");

        assert_eq!(plan.arrays.len(), 2);
        assert_eq!(plan.total_bytes, 24);
    }

    #[test]
    fn word_aligns_and_reverses_short_character_arrays() {
        let locals = vec![
            byte_array("first", Type::Char, 3),
            byte_array("second", Type::Char, 3),
        ];

        let function = function_with_locals(locals);
        let plan = plan_structured_frame_arrays(&function).expect("valid byte arrays");

        assert_eq!(plan.total_bytes, 7);
        assert_eq!(
            structured_array_placement_order(&plan.arrays)
                .iter()
                .map(|array| array.name.as_str())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
    }

    #[test]
    fn retains_an_unused_non_byte_padding_array() {
        let locals = vec![byte_array("words", Type::UnsignedInt, 4)];

        let function = function_with_locals(locals);
        let plan = plan_structured_frame_arrays(&function).expect("unused padding");

        assert_eq!(plan.total_bytes, 16);
    }

    #[test]
    fn retains_a_used_typed_array_for_element_lowering() {
        let locals = vec![byte_array("words", Type::UnsignedInt, 4)];
        let mut function = function_with_locals(locals);
        function
            .statements
            .push(Statement::Expression(Expression::Variable("words".into())));

        let plan = plan_structured_frame_arrays(&function).expect("typed array");
        assert_eq!(plan.total_bytes, 16);
    }

    #[test]
    fn retains_an_aggregate_array_larger_than_one_byte_of_metadata() {
        let locals = vec![byte_array(
            "nodes",
            Type::Struct { size: 8, align: 4 },
            33,
        )];

        let function = function_with_locals(locals);
        let plan = plan_structured_frame_arrays(&function).expect("large node stack");
        assert_eq!(plan.total_bytes, 264);
    }

    #[test]
    fn declines_widened_alignment_until_the_array_base_is_known() {
        let mut buffer = byte_array("buffer", Type::UnsignedChar, 2048);
        buffer.attribute_alignment = Some(32);
        let function = function_with_locals(vec![buffer]);

        assert!(plan_structured_frame_arrays(&function).is_none());
    }

    #[test]
    fn drops_storage_for_an_entire_dead_initialized_array_group() {
        let mut date = byte_array("date", Type::UnsignedChar, 32);
        date.data_bytes = Some(vec![0]);
        let mut time = byte_array("time", Type::UnsignedChar, 32);
        time.data_bytes = Some(vec![0]);

        let function = function_with_locals(vec![date, time]);
        let plan = plan_structured_frame_arrays(&function).expect("initialized arrays");

        assert!(plan.arrays.is_empty());
        assert_eq!(plan.image_sources.len(), 2);
        assert_eq!(plan.total_bytes, 0);
    }

    #[test]
    fn separates_partially_dead_images_from_live_frame_storage() {
        let mut date = byte_array("date", Type::UnsignedChar, 32);
        date.data_bytes = Some(vec![0]);
        let mut time = byte_array("time", Type::UnsignedChar, 32);
        time.data_bytes = Some(vec![0]);
        let mut buffer = byte_array("buffer", Type::UnsignedChar, 256);
        buffer.data_bytes = Some(vec![0]);
        let mut function = function_with_locals(vec![date, time, buffer]);
        function
            .statements
            .push(Statement::Expression(Expression::Variable("date".into())));
        function
            .statements
            .push(Statement::Expression(Expression::Variable("buffer".into())));

        let plan = plan_structured_frame_arrays(&function).expect("initialized arrays");

        assert_eq!(
            plan.arrays
                .iter()
                .map(|array| array.name.as_str())
                .collect::<Vec<_>>(),
            ["date", "buffer"]
        );
        assert_eq!(plan.image_sources.len(), 3);
        assert_eq!(plan.total_bytes, 288);
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

        let ordered = structured_array_placement_order(&arrays);

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
            structured_array_placement_order(&[&buffer])[0].name,
            "buffer"
        );
    }
}
