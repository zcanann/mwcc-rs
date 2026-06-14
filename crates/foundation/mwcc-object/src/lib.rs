//! ELF32 big-endian PowerPC relocatable-object writer.
//!
//! We own the object bytes deliberately: decomp tooling keys on exact section
//! ordering, symbol order, alignment, relocations, and the Metrowerks
//! `.comment`/`.mwcats` records, so the container is reproduced byte-for-byte
//! just like the `.text`. `lib.rs` exposes the input shape and the entry point;
//! the assembly lives in [`writer`].

mod writer;

/// The inputs for one translation unit's object: the source file name (for the
/// `FILE` symbol), the function name, and its encoded `.text`.
pub struct ObjectInput<'a> {
    pub source_name: &'a str,
    pub function_name: &'a str,
    pub text: &'a [u8],
    /// The compiler version being reproduced (e.g. `(2, 4, 2)`); stamped into the
    /// Metrowerks `.comment` record.
    pub version: (u8, u8, u8),
    /// The compiler build number; a `.comment` format marker depends on it.
    pub build: u16,
}

/// Write a relocatable object for a single function.
pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    writer::write_object(input)
}
