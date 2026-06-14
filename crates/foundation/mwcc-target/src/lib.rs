//! The PowerPC (Gekko) target: register file and the EABI calling convention
//! mwcceppc follows. Pure description — no code generation lives here. `lib.rs`
//! re-exports the modules.

mod eabi;
mod register;

pub use eabi::Eabi;
pub use register::{Register, RegisterClass};
