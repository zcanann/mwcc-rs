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
        if array.is_static
            || array.initializer.is_some()
            || array.data_bytes.is_some()
        {
            return None;
        }
        let element_bytes = match array.declared_type {
            Type::Struct { size, .. } => size,
            value_type => u32::from(value_type.width() / 8),
        };
        let bytes = element_bytes
            .checked_mul(u32::from(array.array_length?))
            .filter(|bytes| *bytes != 0 && *bytes <= u32::from(u8::MAX))?;
        total_bytes = total_bytes.checked_add(i16::try_from(bytes).ok()?)?;
    }
    Some(StructuredFrameArrays {
        arrays,
        total_bytes,
    })
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
}
