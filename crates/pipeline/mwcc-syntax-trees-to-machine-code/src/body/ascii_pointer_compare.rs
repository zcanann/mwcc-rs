//! Pointer-adjusting ASCII byte comparison loops.
//!
//! This family comes from reconstructed source whose case adjustment is
//! pointer arithmetic, not byte arithmetic. Preserve those parsed semantics:
//! silently normalizing it into a conventional `strcasecmp` would produce
//! different memory reads and therefore different code.

#[allow(unused_imports)]
use super::*;

struct AsciiPointerCompare<'a> {
    first: &'a str,
    second: &'a str,
}

mod emit;
mod recognize;

#[cfg(test)]
mod tests;
