//! Source-level types in the supported C subset.

/// The pointed-to type of a pointer. Kept a flat, `Copy` enum (no boxing) so
/// `Type` stays `Copy`; pointer-to-pointer and pointer-to-aggregate arrive with
/// the wider type system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointee {
    Int,
    UnsignedInt,
    Char,
    UnsignedChar,
    Short,
    UnsignedShort,
    Float,
}

impl Pointee {
    /// The element type as a [`Type`].
    pub fn element(self) -> Type {
        match self {
            Pointee::Int => Type::Int,
            Pointee::UnsignedInt => Type::UnsignedInt,
            Pointee::Char => Type::Char,
            Pointee::UnsignedChar => Type::UnsignedChar,
            Pointee::Short => Type::Short,
            Pointee::UnsignedShort => Type::UnsignedShort,
            Pointee::Float => Type::Float,
        }
    }
    /// Size in bytes of one element (for indexing).
    pub fn size(self) -> u8 {
        self.element().width() / 8
    }
}

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
    /// A pointer to a scalar, e.g. `int*`.
    Pointer(Pointee),
    /// A pointer to a struct. The struct's layout is resolved by the parser (it
    /// bakes member offsets into [`crate::Expression::Member`]), so codegen only
    /// needs to know this is a general 32-bit address.
    StructPointer,
}

impl Type {
    /// Whether this is a signed integer (affects e.g. `>>` and narrowing). A
    /// pointer is an unsigned address.
    pub fn is_signed(self) -> bool {
        matches!(self, Type::Int | Type::Char | Type::Short)
    }

    /// Integer width in bits (8/16/32); 32 for non-narrow types and pointers.
    pub fn width(self) -> u8 {
        match self {
            Type::Char | Type::UnsignedChar => 8,
            Type::Short | Type::UnsignedShort => 16,
            _ => 32,
        }
    }
}
