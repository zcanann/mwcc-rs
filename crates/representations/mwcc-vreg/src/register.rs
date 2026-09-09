//! Registers in the vreg-IR: the value class, virtual registers, and the operand
//! reference that is either virtual (the allocator assigns it) or already pinned
//! to a physical register (an ABI slot or the scratch).

use mwcc_machine_code::RegisterField;

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

/// Fields below this value name physical registers. Larger fields carry
/// virtual IDs until allocation; the selected representation keeps the full
/// ID instead of limiting a function to one byte's worth of temporaries.
pub const VIRTUAL_BASE: RegisterField = 32;

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
    pub fn from_field(value: RegisterField, class: Class) -> Reg {
        if value >= VIRTUAL_BASE {
            Reg::Virtual(VirtualRegister::new(value - VIRTUAL_BASE, class))
        } else {
            Reg::Physical(value as u8)
        }
    }

    /// Encode back into a selected instruction field. Panics if a virtual ID exceeds
    /// the field's capacity — an honest ceiling, not silent truncation.
    pub fn to_field(self) -> RegisterField {
        match self {
            Reg::Physical(number) => RegisterField::from(number),
            Reg::Virtual(register) => {
                VIRTUAL_BASE.checked_add(register.id)
                    .expect("virtual register ID exceeds selected register field capacity")
            }
        }
    }

    /// Whether a register-field value denotes a virtual register.
    pub fn is_virtual_field(value: RegisterField) -> bool {
        value >= VIRTUAL_BASE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_fields_preserve_ids_beyond_one_byte_in_both_register_files() {
        for class in [Class::General, Class::Float] {
            for id in [0, 223, 224, 255, 256, 65_536, u32::MAX - VIRTUAL_BASE] {
                let register = Reg::Virtual(VirtualRegister::new(id, class));
                assert_eq!(Reg::from_field(register.to_field(), class), register);
                assert!(Reg::is_virtual_field(register.to_field()));
            }
            for number in 0..32 {
                let register = Reg::Physical(number);
                assert_eq!(Reg::from_field(register.to_field(), class), register);
            }
        }
    }

    #[test]
    #[should_panic(expected = "virtual register ID exceeds selected register field capacity")]
    fn an_unrepresentable_id_cannot_alias_a_physical_register() {
        Reg::general(u32::MAX).to_field();
    }

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
