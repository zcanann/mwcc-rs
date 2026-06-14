//! Source-level types in the supported C subset.

/// A source-level type in the supported subset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Int,
    UnsignedInt,
    Char,
    UnsignedChar,
    Short,
    UnsignedShort,
    Float,
    Void,
}

impl Type {
    /// Whether this is a signed integer (affects e.g. `>>` and narrowing).
    pub fn is_signed(self) -> bool {
        matches!(self, Type::Int | Type::Char | Type::Short)
    }

    /// Integer width in bits (8/16/32); 32 for non-narrow types.
    pub fn width(self) -> u8 {
        match self {
            Type::Char | Type::UnsignedChar => 8,
            Type::Short | Type::UnsignedShort => 16,
            _ => 32,
        }
    }
}
