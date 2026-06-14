//! The machine-code representation: a sequence of PowerPC (Gekko) instructions
//! with their encodings.
//!
//! Instructions are structured (not raw words) so the register allocator and
//! instruction scheduler — the phases where byte-matching is won — can inspect
//! and rewrite them before the final encoding. `lib.rs` only wires the modules
//! together; the work lives in them.

mod encoding;
mod frame;
mod function;
mod instruction;
mod relocation;

pub use frame::FrameInfo;
pub use function::MachineFunction;
pub use instruction::Instruction;
pub use relocation::{Relocation, RelocationKind, RelocationTarget};
