//! The compiler-build registry and per-build codegen profiles.
//!
//! Byte-matching is per-build. A [`CompilerBuild`] identifies an mwcceppc build;
//! its [`CodegenProfile`] says how that build's code generation differs from the
//! GameCube 2.4.x mainline. Empirically (differential oracle + cross-build
//! diffing over ~320 forms) builds 53..=108 share one code generator with two
//! observable knobs — plain-`char` signedness and one int->float scheduling
//! choice — so most builds use the mainline profile and the two outliers each
//! override a single method.

mod build;
mod profile;

pub use build::*;
pub use profile::*;
