//! The PowerPC (Gekko) target: register file and the EABI calling convention
//! mwcceppc follows. Pure description — no code generation lives here.

/// The two register classes v0 cares about. Special-purpose registers (LR, CR,
/// CTR) arrive with control flow and calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterClass {
    /// General-purpose registers r0..r31.
    General,
    /// Floating-point registers f0..f31.
    Float,
}

/// A physical register: a class plus its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register {
    pub class: RegisterClass,
    pub number: u8,
}

impl Register {
    pub const fn general(number: u8) -> Self {
        Register { class: RegisterClass::General, number }
    }
    pub const fn float(number: u8) -> Self {
        Register { class: RegisterClass::Float, number }
    }
}

impl std::fmt::Display for Register {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = match self.class {
            RegisterClass::General => 'r',
            RegisterClass::Float => 'f',
        };
        write!(formatter, "{prefix}{}", self.number)
    }
}

/// The PowerPC EABI calling convention as mwcceppc applies it.
///
/// Integer arguments land in r3, r4, r5, …; floating-point arguments in
/// f1, f2, f3, …. Integer results return in r3, float results in f1.
pub struct Eabi;

impl Eabi {
    /// First general-purpose argument register (r3).
    pub const FIRST_GENERAL_ARGUMENT: u8 = 3;
    /// First float argument register (f1).
    pub const FIRST_FLOAT_ARGUMENT: u8 = 1;

    pub fn general_result() -> Register {
        Register::general(3)
    }
    pub fn float_result() -> Register {
        Register::float(1)
    }
}
