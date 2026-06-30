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
    Double,
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
            Pointee::Double => Type::Double,
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
    /// Double-precision float (8 bytes). Shares the FPR class with `Float`.
    Double,
    /// A 64-bit signed integer, held in a big-endian general-register PAIR — the
    /// HIGH word in the lower-numbered register (e.g. `r3:r4` is high:low). Arithmetic
    /// uses carry-aware pairs (`addc`/`adde`, `subfc`/`sube`).
    LongLong,
    /// A 64-bit unsigned integer; same register-pair representation as [`Type::LongLong`].
    UnsignedLongLong,
    Void,
    /// A pointer to a scalar, e.g. `int*`.
    Pointer(Pointee),
    /// A pointer to a struct. The struct's layout is resolved by the parser (it
    /// bakes member offsets into [`crate::Expression::Member`]), so codegen only
    /// needs to know this is a general 32-bit address — plus `element_size`, the
    /// struct's byte size, so pointer arithmetic (`p + n`, `p++`) can scale by it.
    /// `element_size` is 0 for an opaque struct or a function pointer (which reuses
    /// this variant); those defer scaled arithmetic rather than mis-scale.
    StructPointer { element_size: u16 },
    /// A struct *value* (passed/declared by value), carrying its byte size and
    /// alignment (the max member alignment, NOT the size). Used so far for a
    /// frame-resident struct local — `struct S v;` gets a stack slot of this size,
    /// aligned to `align`, and `&v` is its address.
    Struct { size: u16, align: u8 },
}

impl Type {
    /// Whether this is a signed integer (affects e.g. `>>` and narrowing). A
    /// pointer is an unsigned address.
    pub fn is_signed(self) -> bool {
        matches!(self, Type::Int | Type::Char | Type::Short | Type::LongLong)
    }

    /// Width in bits: 8/16/32 for integers and pointers, 64 for a `double` (used
    /// for its 8-byte size). Floats are never "narrow".
    pub fn width(self) -> u8 {
        match self {
            Type::Char | Type::UnsignedChar => 8,
            Type::Short | Type::UnsignedShort => 16,
            Type::Double | Type::LongLong | Type::UnsignedLongLong => 64,
            _ => 32,
        }
    }
}
