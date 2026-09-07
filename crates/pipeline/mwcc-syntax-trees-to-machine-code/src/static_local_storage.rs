//! Storage alignment for source-declared function statics.
//!
//! Local arrays and records share the aggregate promotion policy. Mainline O0
//! keeps natural alignment even in small data, unlike file-scope scalar arrays.

use mwcc_syntax_trees::Type;
use mwcc_versions::{ArrayAlignmentStyle, CompilerConfig, Optimization};

pub(super) fn element_size(declared_type: Type) -> u32 {
    match declared_type {
        Type::Struct { size, .. } => size,
        ty => u32::from(ty.width()) / 8,
    }
}

pub(super) fn alignment(
    declared_type: Type,
    array_length: Option<u16>,
    requested_alignment: Option<u16>,
    config: CompilerConfig,
) -> u32 {
    let element_size = element_size(declared_type);
    let natural = match declared_type {
        Type::Struct { align, .. } => u32::from(align),
        _ => element_size,
    };
    let aggregate = array_length.is_some() || matches!(declared_type, Type::Struct { .. });
    let alignment = if aggregate {
        let size = element_size * array_length.map_or(1, u32::from);
        match config.build.profile.array_alignment_style() {
            ArrayAlignmentStyle::NaturalAtO0 if config.flags.optimization == Optimization::O0 => {
                natural
            }
            ArrayAlignmentStyle::SizeMultipleOfEight if size != 0 && size % 8 == 0 => {
                natural.max(8)
            }
            _ => natural.max(4),
        }
    } else {
        match declared_type {
            Type::Char | Type::UnsignedChar => 1,
            _ => natural.max(4),
        }
    };
    // Build 163 treats an explicit aggregate alignment as an override, even
    // when it reduces natural or word alignment. Later builds use a minimum.
    if aggregate && config.build.profile.array_alignment_style() == ArrayAlignmentStyle::Word {
        requested_alignment.map_or(alignment, |value| u32::from(value).max(1))
    } else {
        alignment.max(requested_alignment.map_or(1, u32::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_versions::{GC_1_1, GC_2_6, GC_3_0A3};

    #[test]
    fn local_aggregates_use_natural_alignment_at_mainline_o0() {
        let mut config = CompilerConfig::new(GC_2_6);
        config.flags.optimization = Optimization::O0;
        assert_eq!(alignment(Type::Char, Some(3), None, config), 1);
        assert_eq!(
            alignment(Type::Struct { size: 3, align: 1 }, None, None, config),
            1
        );
    }

    #[test]
    fn new_record_storage_promotes_size_without_changing_member_layout() {
        let config = CompilerConfig::new(GC_3_0A3);
        assert_eq!(
            alignment(Type::Struct { size: 8, align: 4 }, None, None, config),
            8
        );
        assert_eq!(
            alignment(Type::Struct { size: 12, align: 4 }, None, None, config),
            4
        );
        assert_eq!(alignment(Type::Char, None, None, config), 1);
        assert_eq!(alignment(Type::Char, Some(1), None, config), 4);
    }

    #[test]
    fn explicit_alignment_can_reduce_legacy_aggregate_storage() {
        assert_eq!(
            alignment(Type::Int, Some(2), Some(2), CompilerConfig::new(GC_1_1)),
            2
        );
        assert_eq!(
            alignment(Type::Int, Some(2), Some(2), CompilerConfig::new(GC_2_6)),
            4
        );
        assert_eq!(
            alignment(Type::Int, Some(2), Some(16), CompilerConfig::new(GC_3_0A3)),
            16
        );
    }
}
