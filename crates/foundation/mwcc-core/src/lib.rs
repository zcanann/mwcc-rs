//! Shared vocabulary for the compiler: diagnostics and source positions.
//!
//! This crate holds no pipeline logic — only types every phase agrees on.
//! `lib.rs` re-exports the modules.

mod diagnostic;
mod span;

pub use diagnostic::{Compilation, Diagnostic};
pub use span::{SourcePosition, SourceSpan};
