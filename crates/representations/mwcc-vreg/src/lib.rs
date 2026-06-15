//! The virtual-register IR and the register allocator (Phase D — the keystone).
//!
//! Instruction selection chooses *what* to compute and in what order; it should
//! not choose *which physical register* each value lives in. This crate is where
//! that choice moves to. Selection emits values as [`VirtualRegister`]s (with a
//! [`Class`]); the allocator assigns each a physical home under the target's
//! [`RegisterConstraints`], honoring liveness so values that are live at the same
//! time never share a register. The physical [`mwcc_machine_code::Instruction`]
//! stream is the *post-allocation* form — this crate sits one stage before it.
//!
//! Today this is the foundation, landed unwired (the existing code generator
//! still chooses registers inline, all builds byte-exact). It is exercised by its
//! own tests. The migration that routes selection through it — slice by slice,
//! keeping every build byte-exact — is described in `docs/register-allocator.md`.

mod allocator;
mod constraints;
mod description;
mod liveness;
mod register;
mod schedule;

pub use allocator::*;
pub use constraints::*;
pub use description::*;
pub use liveness::*;
pub use register::*;
pub use schedule::*;
