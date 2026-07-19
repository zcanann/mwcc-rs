//! The compiler-build registry and per-build codegen profiles.
//!
//! Byte-matching is per-build. A [`CompilerBuild`] identifies an mwcceppc build;
//! its [`CodegenProfile`] says how that build's code generation differs from the
//! GameCube 2.4.x mainline. Differential oracle and cross-build measurements
//! resolve each observed transition into a narrow profile decision, keeping one
//! shared generator while preserving changes in instruction selection, register
//! allocation, and scheduling between builds 53 and 108.

mod behavior;
mod build;
mod config;
mod flags;
mod profile;

pub use behavior::*;
pub use build::*;
pub use config::*;
pub use flags::*;
pub use profile::*;
