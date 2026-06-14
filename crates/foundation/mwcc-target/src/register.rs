//! The PowerPC (Gekko) register file.

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
