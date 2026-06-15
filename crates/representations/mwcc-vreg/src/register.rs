//! Registers in the vreg-IR: the value class, virtual registers, and the operand
//! reference that is either virtual (the allocator assigns it) or already pinned
//! to a physical register (an ABI slot or the scratch).

/// Which register file a value lives in. PowerPC keeps integers/pointers in the
/// general-purpose registers and floating-point in a separate file; a value never
/// crosses, so the allocator draws each class from its own pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    General,
    Float,
}

/// A value produced during instruction selection that has not yet been given a
/// physical home. Carries its [`Class`] so the allocator assigns it from the
/// right pool. The `id` is unique within a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualRegister {
    pub id: u32,
    pub class: Class,
}

impl VirtualRegister {
    pub fn new(id: u32, class: Class) -> Self {
        VirtualRegister { id, class }
    }
}

/// A register reference in a selected instruction: either a virtual register the
/// allocator must place, or a physical register already fixed by the ABI (a
/// parameter/return slot, the stack pointer) or by being the scratch. Pinned
/// references constrain allocation — a virtual register live across a pinned use
/// of `r3` cannot itself be `r3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reg {
    /// Assigned by the allocator.
    Virtual(VirtualRegister),
    /// Fixed before allocation (ABI slot, stack pointer, or scratch).
    Physical(u8),
}

/// Register-field values below this are physical registers (0..=31, in either
/// file); from here up, a value encodes a virtual register (`id = value -
/// VIRTUAL_BASE`) directly in the u8 instruction field that selection emits,
/// before allocation resolves it. This is the transitional bridge that lets the
/// existing `Instruction` (with `u8` fields) carry virtuals without being
/// parameterized over its register type; it bounds a function to 224 virtuals
/// per class — ample for the migration's slices, widened later if a real
/// function needs more (by moving to `Instruction<Reg>` or wider fields).
pub const VIRTUAL_BASE: u8 = 32;

impl Reg {
    pub fn general(id: u32) -> Self {
        Reg::Virtual(VirtualRegister::new(id, Class::General))
    }

    pub fn float(id: u32) -> Self {
        Reg::Virtual(VirtualRegister::new(id, Class::Float))
    }

    /// The virtual register this refers to, if it is not yet pinned.
    pub fn virtual_register(self) -> Option<VirtualRegister> {
        match self {
            Reg::Virtual(register) => Some(register),
            Reg::Physical(_) => None,
        }
    }

    /// The physical register, once pinned (or after allocation resolves it).
    pub fn physical(self) -> Option<u8> {
        match self {
            Reg::Physical(number) => Some(number),
            Reg::Virtual(_) => None,
        }
    }

    /// Decode a register-field value of a known [`Class`]: physical below
    /// [`VIRTUAL_BASE`], virtual at or above it.
    pub fn from_field(value: u8, class: Class) -> Reg {
        if value >= VIRTUAL_BASE {
            Reg::Virtual(VirtualRegister::new((value - VIRTUAL_BASE) as u32, class))
        } else {
            Reg::Physical(value)
        }
    }

    /// Encode back into a u8 instruction field. Panics if a virtual id exceeds
    /// the field's capacity — an honest ceiling, not silent truncation.
    pub fn to_field(self) -> u8 {
        match self {
            Reg::Physical(number) => number,
            Reg::Virtual(register) => {
                let encoded = VIRTUAL_BASE as u32 + register.id;
                assert!(encoded <= u8::MAX as u32, "virtual register id {} exceeds the u8 field ceiling", register.id);
                encoded as u8
            }
        }
    }

    /// Whether a register-field value denotes a virtual register.
    pub fn is_virtual_field(value: u8) -> bool {
        value >= VIRTUAL_BASE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_reference_exposes_its_register_not_a_physical_one() {
        let reg = Reg::general(7);
        assert_eq!(reg.virtual_register(), Some(VirtualRegister::new(7, Class::General)));
        assert_eq!(reg.physical(), None);
    }

    #[test]
    fn a_pinned_reference_exposes_a_physical_number_not_a_virtual_one() {
        let reg = Reg::Physical(3);
        assert_eq!(reg.physical(), Some(3));
        assert_eq!(reg.virtual_register(), None);
    }

    #[test]
    fn classes_keep_general_and_float_virtuals_distinct() {
        assert_ne!(Reg::general(1), Reg::float(1));
    }
}
