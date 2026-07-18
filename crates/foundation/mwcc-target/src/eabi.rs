//! The PowerPC EABI calling convention as mwcceppc applies it.

use crate::register::Register;

/// The PowerPC EABI calling convention as mwcceppc applies it.
///
/// Integer arguments land in r3, r4, r5, …; floating-point arguments in
/// f1, f2, f3, …. Integer results return in r3, float results in f1.
pub struct Eabi;

impl Eabi {
    /// First general-purpose argument register (r3).
    pub const FIRST_GENERAL_ARGUMENT: u8 = 3;
    /// Last general-purpose argument register (r10).
    pub const LAST_GENERAL_ARGUMENT: u8 = 10;
    /// First float argument register (f1).
    pub const FIRST_FLOAT_ARGUMENT: u8 = 1;

    pub fn general_result() -> Register {
        Register::general(3)
    }
    pub fn float_result() -> Register {
        Register::float(1)
    }
}
