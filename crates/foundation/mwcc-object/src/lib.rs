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
    /// File-scope variables *defined* in this unit (not `extern`), in declaration
    /// order. Each becomes a defined symbol in a data section; uninitialized ones
    /// live in `.sbss`. mwcc lays them out in *reverse* declaration order.
    pub data_objects: Vec<DataObject<'a>>,
    /// Whether the small-data area is in use (the default). With `-sdata 0` it is
    /// off, and the data sections are named `.bss`/`.data` instead of
    /// `.sbss`/`.sdata` (identical type/flags/alignment otherwise).
    pub small_data: bool,
    /// Names of `static inline` asm functions skipped from the source, in
    /// declaration order. Each becomes a local *undefined* symbol (mwcc keeps
    /// inline-asm helpers as deferred symbols even when unused).
    pub inline_asm_symbols: &'a [String],
}

/// A file-scope variable defined in this object: its name, byte size, natural
/// alignment, and optional initialized bytes. `Some(bytes)` (any non-zero
/// initializer) places it in `.sdata` with those bytes; `None` leaves it in
/// `.sbss` (NOBITS, zero-initialized). `.sdata` lays out in forward declaration
/// order, `.sbss` in reverse — matching mwcc.
///
/// A `const` object is read-only: it lands in `.sdata2` (≤ 8 bytes) or `.rodata`
/// (larger) instead, both in forward declaration order.
pub struct DataObject<'a> {
    pub name: &'a str,
    pub size: u32,
    pub alignment: u32,
    pub initial_bytes: Option<Vec<u8>>,
    pub is_const: bool,
    /// A `static` global is file-local: same section routing, but its symbol binds
    /// LOCAL (and is emitted among the local symbols, not the global run).
    pub is_static: bool,
    /// True when the object is an EXPLICITLY zero-initialized global (`int a = 0;`,
    /// `int *p = 0;`) as opposed to an uninitialized one (`int a;`). Both live in
    /// `.sbss`/`.bss` with no file bytes, but mwcc lays the explicit-zero ones out in
    /// DECLARATION order ahead of the uninitialized ones (which reverse). See the
    /// `.sbss` placement in the writer.
    pub is_explicit_zero: bool,
    /// `R_PPC_ADDR32` relocations this object's bytes carry — a pointer global
    /// initialized with the address of another symbol (`int *p = &g;`). Each patches
    /// 4 bytes at `offset` to `target + addend`.
    pub relocations: Vec<DataRelocation>,
}

/// An `R_PPC_ADDR32` relocation inside a data object: 4 bytes at `offset` resolve
/// to `target`'s address plus `addend`.
#[derive(Clone)]
pub struct DataRelocation {
    pub offset: u32,
    pub target: String,
    pub addend: i32,
}

/// One function's contribution to the object: its name, encoded `.text`, and the
/// relocations/constants/frame it owns. Relocation offsets are *relative to this
/// function's start*; the writer rebases them by the function's `.text` offset.
pub struct FunctionObject<'a> {
    pub name: &'a str,
    /// A `static` (file-local) function — emitted with a LOCAL `STT_FUNC` symbol.
    pub is_static: bool,
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
    /// The count of NEW (non-reused) strings this function contributes to the unit's
    /// `@N` string pool. They are numbered at the FRONT of this function's `@N` block
    /// (before its constants and unwind entries), so the writer advances by this first.
    pub string_count: u32,
    /// The `@N` names of those NEW strings, in front-of-block order. The writer emits a
    /// LOCAL object symbol for each at the FRONT of this function's `@N` block (its bytes
    /// and section/offset come from the matching `.sdata`/`.data` data object), so the
    /// string symbol interleaves per-function with the constants/unwind entries the way
    /// mwcc lays them out — not grouped in the data run.
    pub string_names: Vec<String>,
    /// A dense `switch`'s jump table. The writer materializes it as an anonymous
    /// `@N` object in `.data`, fills the per-entry `ADDR32` relocations to this
    /// function, and resolves this function's `JumpTable` `.text` relocations.
    pub jump_table: Option<JumpTable>,
    /// The names this function references (globals/callees) in mwcc's symbol-table
    /// order — an AST traversal, not `.text` reference order. The writer assigns
    /// this function's external/global symbols in this order, with a relocation-
    /// order fallback for anything not listed.
    pub symbol_order: Vec<String>,
    /// Callees this function references that were IMPLICITLY declared (K&R first-use, no
    /// prototype). mwcc creates their symbols at the call site inside the body, so the
    /// writer emits them AFTER this function's own symbol rather than before it.
    pub implicit_external_callees: Vec<String>,
}

/// A dense `switch`'s jump table — one `.text` body offset per index, plus how far
/// the table's anonymous `@N` symbol sits past the function's running counter.
pub struct JumpTable {
    pub entries: Vec<u32>,
    pub anonymous_offset: u32,
}

/// What a `.text` relocation points at.
pub enum RelocationTarget {
    /// An external symbol defined elsewhere (a global or callee).
    External(String),
    /// An entry in this object's constant pool, by index.
    Constant(usize),
    /// This function's own jump table (the anonymous `@N` object in `.data`).
    JumpTable,
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
