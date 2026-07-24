//! Automatic byte-array planning for structured stack frames.
//!
//! Source padding and scratch buffers remain distinct frame slots even when
//! they are unused.  Keeping their validation and byte accounting here lets
//! the structured body owner compose any number of them with aggregate slots.

#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredFrameArrays<'a> {
    pub(super) arrays: Vec<&'a LocalDeclaration>,
    pub(super) total_bytes: i16,
}

pub(super) fn plan_structured_frame_arrays(
    locals: &[LocalDeclaration],
) -> Option<StructuredFrameArrays<'_>> {
    let arrays: Vec<_> = locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .collect();
    let mut total_bytes = 0i16;
    for array in &arrays {
        if array.is_static
            || array.initializer.is_some()
            || array.data_bytes.is_some()
            || !matches!(array.declared_type, Type::Char | Type::UnsignedChar)
        {
            return None;
        }
        let bytes = u16::from(array.declared_type.width() / 8)
            .checked_mul(array.array_length?)
            .filter(|bytes| *bytes != 0 && *bytes <= u16::from(u8::MAX))?;
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

        let plan = plan_structured_frame_arrays(&locals).expect("valid byte arrays");

        assert_eq!(plan.arrays.len(), 2);
        assert_eq!(plan.total_bytes, 24);
    }

    #[test]
    fn rejects_non_byte_automatic_arrays() {
        let locals = vec![byte_array("words", Type::UnsignedInt, 4)];

        assert!(plan_structured_frame_arrays(&locals).is_none());
    }
}
