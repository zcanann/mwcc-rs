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
pub use function::{
    AnonymousRodata, DeferredDisplacement, DeferredDisplacementTarget, DebugVariable,
    DebugVariableLocation, FixedFillAddressSchedule, JumpTable, LaterFillEntrySchedule, MachineFunction, PoolConstant, StaticLocal,
};
pub use instruction::Instruction;
pub use relocation::{Relocation, RelocationKind, RelocationTarget};

/// A selected register field: physical numbers occupy 0..32, and allocation
/// identities use the remaining space until the allocation pass resolves them.
pub type RegisterField = u32;
