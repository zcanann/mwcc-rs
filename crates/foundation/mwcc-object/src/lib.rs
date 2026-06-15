//! ELF32 big-endian PowerPC relocatable-object writer.
//!
//! We own the object bytes deliberately: decomp tooling keys on exact section
//! ordering, symbol order, alignment, relocations, and the Metrowerks
//! `.comment`/`.mwcats` records, so the container is reproduced byte-for-byte
//! just like the `.text`. An object holds one or more functions sharing a single
//! `.text`, constant pool, and unwind sections. `lib.rs` exposes the input shape
//! and the entry point; the assembly lives in [`writer`].

mod writer;

/// The inputs for one translation unit's object: the source file name (for the
/// `FILE` symbol), the compiler identity, and one [`FunctionObject`] per function
/// definition in source order.
pub struct ObjectInput<'a> {
    pub source_name: &'a str,
    /// The compiler version being reproduced (e.g. `(2, 4, 2)`); stamped into the
    /// Metrowerks `.comment` record.
    pub version: (u8, u8, u8),
    /// The compiler build number; a `.comment` format marker depends on it.
    pub build: u16,
    /// One entry per function definition, in source order. They share one `.text`,
    /// one `.sdata2` constant pool, one `.mwcats.text` (a record each), and the
    /// `extab`/`extabindex` unwind sections.
    pub functions: Vec<FunctionObject<'a>>,
}

/// One function's contribution to the object: its name, encoded `.text`, and the
/// relocations/constants/frame it owns. Relocation offsets are *relative to this
/// function's start*; the writer rebases them by the function's `.text` offset.
pub struct FunctionObject<'a> {
    pub name: &'a str,
    pub text: &'a [u8],
    /// `.text` relocations against external symbols (globals, callees) or pooled
    /// constants. Offsets are relative to this function's start.
    pub relocations: Vec<TextRelocation>,
    /// Constants this function loads from `.sdata2`. Each becomes an anonymous
    /// `@N` object.
    pub constants: Vec<Sdata2Constant>,
    /// Unwind-table layout for a stack-frame function; `None` for a pure leaf.
    pub frame: Option<FrameLayout>,
    /// How far this function advances the anonymous `@N` counter *before* its
    /// constants are numbered: +1 for a float<->int conversion, +3 for a float
    /// conditional branch. mwcceppc consumes these counter slots for the
    /// function's internal labels.
    pub anonymous_bump: u32,
}

/// What a `.text` relocation points at.
pub enum RelocationTarget {
    /// An external symbol defined elsewhere (a global or callee).
    External(String),
    /// An entry in this object's constant pool, by index.
    Constant(usize),
}

/// A `.text` relocation: a byte offset, the ELF relocation type, and its target.
pub struct TextRelocation {
    pub offset: u32,
    pub elf_type: u32,
    pub target: RelocationTarget,
}

/// A constant placed in `.sdata2`: its big-endian bit pattern and byte width
/// (4 for a single-precision float, 8 for a double).
pub struct Sdata2Constant {
    pub bits: u64,
    pub byte_width: u8,
}

/// The `extab`/`extabindex` unwind tables a stack-frame function carries. The
/// header word is encoded by the codegen from the saved-register shape; the
/// writer only places it.
pub struct FrameLayout {
    pub extab_header: u32,
}

/// Write a relocatable object for one or more functions.
pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    writer::write_object(input)
}
