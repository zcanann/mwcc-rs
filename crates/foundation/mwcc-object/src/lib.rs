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
}

/// Write a relocatable object for a single function.
pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    writer::write_object(input)
}
