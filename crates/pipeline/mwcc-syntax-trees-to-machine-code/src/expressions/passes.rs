//! Free load/store instruction helpers and address arithmetic shared across families.

#[allow(unused_imports)]
use super::*;


/// The base variable a memory load addresses through — `a` for `a[i]`, `s` for
/// `s->x`, `p` for `*p`. Used to recognize two loads that share a base register.
pub(crate) fn load_base_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Index { base, .. } | Expression::Member { base, .. } => leaf_name(base),
        Expression::Dereference { pointer } => leaf_name(pointer),
        _ => None,
    }
}

/// The displacement load for a pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`).
pub(crate) fn displacement_load(pointee: Pointee, d: u8, a: u8, offset: i16) -> Compilation<Instruction> {
    Ok(match pointee {
        Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer | Pointee::WordPointer => Instruction::LoadWord { d, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZero { d, a, offset },
        Pointee::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset },
        Pointee::Float => Instruction::LoadFloatSingle { d, a, offset },
        Pointee::Double => Instruction::LoadFloatDouble { d, a, offset },
        // An 8-byte register pair has no single load — the pair path is not built yet.
        Pointee::LongLong | Pointee::UnsignedLongLong => {
            return Err(Diagnostic::error("a long long load through a pointer is not supported yet (roadmap)"))
        }
    })
}

/// The indexed load for a pointee type (`lwzx`/`lbzx`/`lhax`/`lhzx`/`lfsx`).
pub(crate) fn indexed_load(pointee: Pointee, d: u8, a: u8, b: u8) -> Compilation<Instruction> {
    Ok(match pointee {
        Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer | Pointee::WordPointer => Instruction::LoadWordIndexed { d, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZeroIndexed { d, a, b },
        Pointee::Short => Instruction::LoadHalfwordAlgebraicIndexed { d, a, b },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZeroIndexed { d, a, b },
        Pointee::Float => Instruction::LoadFloatSingleIndexed { d, a, b },
        Pointee::Double => Instruction::LoadFloatDoubleIndexed { d, a, b },
        Pointee::LongLong | Pointee::UnsignedLongLong => {
            return Err(Diagnostic::error("a long long load through a pointer is not supported yet (roadmap)"))
        }
    })
}

/// A scalar type as the matching [`Pointee`] (for global loads/stores).
pub(crate) fn pointee_of_type(value_type: Type) -> Option<Pointee> {
    Some(match value_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        // A pointer value is a 4-byte address (stored/loaded with `stw`/`lwz`).
        Type::Pointer(_) | Type::StructPointer { .. } => Pointee::UnsignedInt,
        // `double` storage (8-byte lfd/stfd) is a later stage.
        Type::Double => Pointee::Double,
        // A struct value is not a scalar pointee (it has no single load/store); neither is a
        // long long (an 8-byte register pair loaded/stored as two words).
        Type::Void | Type::Struct { .. } | Type::LongLong | Type::UnsignedLongLong => return None,
    })
}

/// The scaled-arithmetic stride for a pointer type: a struct pointer's element size
/// (so `p + n` advances by whole structs), or `None` for a scalar pointer (which
/// scales by its `pointee` size) and a non-pointer. A zero element size — an opaque
/// struct or a function pointer — yields `None` so arithmetic stays unscaled.
pub(crate) fn pointer_stride(value_type: Type) -> Option<u16> {
    match value_type {
        Type::StructPointer { element_size } if element_size > 1 => Some(element_size),
        _ => None,
    }
}

/// The displacement store for a pointee type (`stw`/`stb`/`sth`/`stfs`).
pub(crate) fn displacement_store(pointee: Pointee, s: u8, a: u8, offset: i16) -> Compilation<Instruction> {
    Ok(match pointee {
        Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer | Pointee::WordPointer => Instruction::StoreWord { s, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        Pointee::Float => Instruction::StoreFloatSingle { s, a, offset },
        Pointee::Double => Instruction::StoreFloatDouble { s, a, offset },
        Pointee::LongLong | Pointee::UnsignedLongLong => {
            return Err(Diagnostic::error("a long long store through a pointer is not supported yet (roadmap)"))
        }
    })
}

/// `*(T *)0xADDR` — a dereference through a constant-address pointer cast (memory-mapped
/// hardware registers, the GX FIFO). Returns the pointee and the absolute address.
pub(crate) fn const_address_pointer(pointer: &Expression) -> Option<(Pointee, u32)> {
    if let Expression::Cast { target_type: Type::Pointer(pointee), operand } = pointer {
        // Integer/char/short pointees only — a float/double const-address access needs an
        // FPR destination and a separate path, so leave those to defer.
        if !matches!(pointee, Pointee::Float | Pointee::Double) {
            return constant_value(operand).map(|address| (*pointee, address as u32));
        }
    }
    None
}

/// Split a 32-bit absolute address into the `lis` high half and the displacement low half,
/// the way mwcc does: the low half is sign-interpreted (so a `lo >= 0x8000` reads back as a
/// negative displacement), and the high half is carry-adjusted to compensate. So
/// `0xCC008000` becomes `lis -13311` + displacement `-32768`.
pub(crate) fn split_address(address: u32) -> (i16, i16) {
    let low = address as i16;
    let high = ((address >> 16) as i16).wrapping_add(if address & 0x8000 != 0 { 1 } else { 0 });
    (high, low)
}

/// The absolute address of any constant-address pointer cast — `(T *)C`, `(struct S *)C`,
/// `(union U *)C` — used as a member base (`(*(struct S *)C).field`) where the access width
/// comes from the member, not the cast. Returns `None` for non-constant or non-pointer casts.
pub(crate) fn const_address_of(pointer: &Expression) -> Option<u32> {
    if let Expression::Cast { target_type, operand } = pointer {
        if matches!(target_type, Type::Pointer(_) | Type::StructPointer { .. }) {
            return constant_value(operand).map(|address| address as u32);
        }
    }
    None
}

/// The indexed store for a pointee type (`stwx`/`stbx`/`sthx`/`stfsx`).
pub(crate) fn indexed_store(pointee: Pointee, s: u8, a: u8, b: u8) -> Compilation<Instruction> {
    Ok(match pointee {
        Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer | Pointee::WordPointer => Instruction::StoreWordIndexed { s, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByteIndexed { s, a, b },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfwordIndexed { s, a, b },
        Pointee::Float => Instruction::StoreFloatSingleIndexed { s, a, b },
        Pointee::Double => Instruction::StoreFloatDoubleIndexed { s, a, b },
        Pointee::LongLong | Pointee::UnsignedLongLong => {
            return Err(Diagnostic::error("a long long store through a pointer is not supported yet (roadmap)"))
        }
    })
}

