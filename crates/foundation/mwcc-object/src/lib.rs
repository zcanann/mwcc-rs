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
    /// `.text` relocations against external symbols (globals, callees).
    pub relocations: Vec<TextRelocation>,
    /// Unwind-table layout for a stack-frame function; `None` for a pure leaf.
    pub frame: Option<FrameLayout>,
}

/// A `.text` relocation: a byte offset, the ELF relocation type, and the
/// (external) symbol it references.
pub struct TextRelocation {
    pub offset: u32,
    pub elf_type: u32,
    pub symbol: String,
}

/// The `extab`/`extabindex` unwind tables a stack-frame function carries. The
/// header word is encoded by the codegen from the saved-register shape; the
/// writer only places it.
pub struct FrameLayout {
    pub extab_header: u32,
}

/// Write a relocatable object for a single function.
pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    writer::write_object(input)
}
