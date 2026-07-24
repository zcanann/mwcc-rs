//! ELF32 big-endian PowerPC relocatable-object writer.
//!
//! We own the object bytes deliberately: decomp tooling keys on exact section
//! ordering, symbol order, alignment, relocations, and the Metrowerks
//! `.comment`/`.mwcats` records, so the container is reproduced byte-for-byte
//! just like the code. An object holds functions split across `.text` and custom
//! code sections while sharing constant pools and unwind sections. `lib.rs` exposes the input shape
//! and the entry point; the assembly lives in [`writer`].

mod debug;
mod function_layout;
mod writer;

pub use debug::{
    DebugLayout, DebugRelocation, DebugRelocationKind, DebugRelocationTarget, DebugSection,
    DebugSections, DebugSymbol, DebugSymbolBinding, DebugSymbolPlacement,
};
pub use function_layout::{
    layout_code_sections, layout_function_placements, layout_functions, CodeSection,
    CodeSectionLayout, FunctionLayout, FunctionPlacement,
};

/// The compiler-specific header fields of Metrowerks' `.comment` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentFormat {
    pub marker: u8,
    pub version: (u8, u8, u8),
    /// Byte 16 of the Metrowerks header records the `-pool` mode: one when
    /// compiler pooling is enabled, zero for `-pool off`.
    pub pooling_enabled: bool,
}

/// Build-specific conventions affecting relocatable-object encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectFormat {
    pub comment: CommentFormat,
    pub emb_sda21_offset: u8,
    /// Required alignment of the code section and each function body.
    pub code_alignment: u32,
    /// Whether the `.sdata2` constant pool carries ELF's `SHF_WRITE` bit.
    pub sdata2_writable: bool,
    pub function_symbol_order: FunctionSymbolOrder,
    /// Weak vtable-owned inline functions keep forward text layout but expose
    /// reverse-slot symbol discovery after ordinary source functions.
    pub weak_vtable_function_symbol_tail: bool,
    /// Deferred code generation completes initialized file-scope declarations
    /// before its reversed function-emission pass, regardless of where those
    /// declarations appeared between source functions.
    pub initialized_globals_before_deferred_functions: bool,
    /// Whether file-scope LOCAL data symbols preserve declaration order across
    /// initialized and zero-filled sections.
    pub local_data_symbols_in_declaration_order: bool,
    /// Whether file-scope static `.sbss` objects form a declaration-order phase
    /// between exported explicit-zero and tentative-definition objects.
    pub small_zero_statics_in_declaration_order: bool,
    /// Whether `...rodata.0` precedes named `.rodata` data symbols.
    pub rodata_anchor_before_data_symbols: bool,
    /// `.comment` attribute flags for `...rodata.0`.
    pub rodata_anchor_comment_flags: u32,
    /// Whether data initializers targeting full `.data`/`.rodata` objects bind
    /// through the corresponding zero-offset section anchor.
    pub data_relocations_use_section_anchors: bool,
    /// `.comment` attribute flags for `...data.0`.
    pub data_anchor_comment_flags: u32,
    pub initial_anonymous_counter: u8,
    pub post_leaf_function_anonymous_bump: u8,
    pub post_framed_function_anonymous_bump: u8,
}

/// When a function's global symbol is registered relative to symbols first
/// discovered while compiling its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionSymbolOrder {
    /// Prototyped references are registered before the function itself.
    ReferencesFirst,
    /// The function is registered before body-created references, except for
    /// the legacy fixed-address-symbol special case.
    FunctionFirst,
    /// Unoptimized modern code registers the current function before its body
    /// references, but a call to a later definition does not hoist that later
    /// function's symbol away from its source-definition position.
    FunctionFirstAtDefinition,
    /// Build 163 with deferred inlining retains function-first body references,
    /// but resolves data already defined in the translation unit before the
    /// current function.
    LegacyDeferred,
    /// Modern deferred codegen registers locally defined function targets before
    /// the current function. Ordinary declared references, including defined
    /// data, retain reference-first registration.
    Deferred,
}

/// The inputs for one translation unit's object: the source file name (for the
/// `FILE` symbol), the compiler identity, and one [`FunctionObject`] per function
/// definition in source order.
pub struct ObjectInput<'a> {
    pub source_name: &'a str,
    /// Compiler-specific `.comment` header fields.
    pub object_format: ObjectFormat,
    /// One entry per function definition, in source order. They share one `.sdata2`
    /// constant pool and unwind tables; each function selects its own code section.
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
    /// Whether compiler-generated functions receive Code Address Table entries
    /// in `.mwcats.text`. Disabled by `-pragma "cats off"`.
    pub emit_mwcats: bool,
    /// Names of `static inline` asm functions skipped from the source, in
    /// declaration order. Each becomes a local *undefined* symbol (mwcc keeps
    /// inline-asm helpers as deferred symbols even when unused).
    pub inline_asm_symbols: &'a [String],
    /// Names of `static` functions whose LOCAL FUNC symbols are created before
    /// deferred per-function code generation. This includes functions address-taken
    /// by data initializers and prototyped functions address-taken by earlier text
    /// (measured: NMWException's handlers and OSAlarm's
    /// `DecrementerExceptionHandler`).
    pub early_static_function_symbols: &'a [String],
    /// Unused section-attributed function prototypes retained as GLOBAL UND
    /// symbols by early compilers, in declaration order.
    pub early_undefined_externals: &'a [String],
    /// Section-attributed function declarations seen before their definitions.
    /// Consecutive asm definitions in this set register as one symbol run.
    pub section_function_declarations: &'a [String],
    /// Section-attributed extern data declarations retained as a declaration
    /// group when any member is first referenced (legacy `_ctors`/`_dtors`).
    pub section_externals: &'a [(String, usize)],
    /// Optional capture pin for interleaved LOCAL data/function symbols.
    pub local_symbol_order: &'a [String],
    /// Optional CodeWarrior DWARF 1 payload and its object-container metadata.
    pub debug: Option<DebugSections>,
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
    /// Alignment used for section layout.
    pub alignment: u32,
    /// Alignment emitted in this symbol's `.comment` record. CodeWarrior keeps
    /// an array's element alignment here even when storage is word-aligned.
    pub comment_alignment: u32,
    pub initial_bytes: Option<Vec<u8>>,
    pub is_const: bool,
    /// Route initialized data to `.data`/`.rodata` even when it is at most eight
    /// bytes, bypassing the ordinary small-data threshold.
    pub force_full_data_section: bool,
    /// A `static` global is file-local: same section routing, but its symbol binds
    /// LOCAL (and is emitted among the local symbols, not the global run).
    pub is_static: bool,
    /// True when the object is an EXPLICITLY zero-initialized global (`int a = 0;`,
    /// `int *p = 0;`) as opposed to an uninitialized one (`int a;`). Both live in
    /// `.sbss`/`.bss` with no file bytes, but mwcc lays the explicit-zero ones out in
    /// DECLARATION order ahead of the uninitialized ones (which reverse). See the
    /// `.sbss` placement in the writer.
    pub is_explicit_zero: bool,
    /// Compiler-assigned sparse `@N` identity for a frontend/optimizer data
    /// temporary. `None` denotes source data or an ordinary pooled literal.
    pub preassigned_anonymous_ordinal: Option<u32>,
    /// `R_PPC_ADDR32` relocations this object's bytes carry — a pointer global
    /// initialized with the address of another symbol (`int *p = &g;`). Each patches
    /// 4 bytes at `offset` to `target + addend`.
    pub relocations: Vec<DataRelocation>,
    /// Non-static functions defined before this object — the writer emits its
    /// symbol at that source position among the function symbol runs.
    pub non_static_functions_before: usize,
    /// ALL function definitions (static included) before this declaration.
    pub functions_before: usize,
    /// A WEAK object symbol (an inline function's emitted static local).
    pub is_weak: bool,
    /// A real function's static local: the owning function index (numbering).
    pub static_local_owner: Option<usize>,
    /// Signed shift a static local's `$N` takes off the owner's base counter
    /// (declaration-position part of the unit's inline pre-bump).
    pub anonymous_adjust: i64,
    /// An explicit output section from `__declspec(section "…")` — overrides the
    /// default section routing (e.g. `.dtors` for a global-destructor reference).
    /// `None` uses the size/const/zero rules.
    pub section: Option<&'a str>,
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
    /// See MachineFunction::static_locals_lead.
    pub static_locals_lead: bool,
    /// See MachineFunction::text_deferred.
    pub text_deferred: bool,
    pub is_weak: bool,
    /// An explicit `__declspec(section "…")` code section (e.g. `.init`), overriding
    /// the default `.text`/`.mwcats.text` placement. `None` = `.text`.
    pub section: Option<&'a str>,
    /// A Metrowerks inline-`asm` function. Its code lands in `.text` like any other,
    /// but mwcc does NOT catalog hand-written asm in `.mwcats.text`, so the writer
    /// omits its mwcats record and relocation.
    pub is_asm: bool,
    /// Inline-`asm` `entry <name>` points: additional GLOBAL `.text` symbols at byte
    /// offsets within this function (`_savefpr_14` …). Each pairs the symbol name with
    /// its byte offset relative to the function start.
    pub entry_points: Vec<(String, u32)>,
    /// Defined under `#pragma force_active on`: the function symbol and its entry
    /// symbols carry a `.comment` attribute (0x00080000).
    pub force_active: bool,
    pub text: &'a [u8],
    /// `.text` relocations against external symbols (globals, callees) or pooled
    /// constants. Offsets are relative to this function's start.
    pub relocations: Vec<TextRelocation>,
    /// Constants this function loads from a compiler-managed read-only pool.
    /// Each becomes an anonymous `@N` object in `.sdata2` or `.rodata`.
    pub constants: Vec<Sdata2Constant>,
    /// Unwind-table layout for a stack-frame function; `None` for a pure leaf.
    pub frame: Option<FrameLayout>,
    /// How far this function advances the anonymous `@N` counter *before* its
    /// constants are numbered: +1 for a float<->int conversion, +3 for a float
    /// conditional branch. mwcceppc consumes these counter slots for the
    /// function's internal labels.
    pub anonymous_bump: u32,
    /// A static function materialized from an IMPLICITLY-declared inline: its
    /// LOCAL symbol emits after its own static locals (not in the early static
    /// run), and call relocations bind the surviving UNDEFINED global ghost.
    pub implicit_local: bool,
    /// A WEAK function materialized from a PLAIN inline — comment flag 0x0d.
    pub weak_inline: bool,
    /// Mid-pool `@N` gaps applied while numbering constants: (constant index,
    /// extra numbers consumed before it).
    pub constant_number_gaps: Vec<(usize, u32)>,
    /// Signed adjustment at the front of this function's pool block. Negative
    /// values account for constants created by an inlined body before the
    /// enclosing function's ordinary anonymous counter position.
    pub constant_number_adjust: i32,
    /// UND externals with no relocation, emitted first among the externals.
    pub phantom_externals: Vec<String>,
    /// `@N` numbers consumed after the constants, before the extab pair.
    pub post_constant_bump: u32,
    /// Function-specific override for the build-wide anonymous-counter gap
    /// after this function's complete block.
    pub post_function_anonymous_bump: Option<u8>,
    /// The count of NEW (non-reused) strings this function contributes to the unit's
    /// `@N` string pool. They are numbered at the FRONT of this function's `@N` block
    /// (before its constants and unwind entries), so the writer advances by this first.
    pub string_count: u32,
    /// See MachineFunction::string_number_after_constants.
    pub string_number_after_constants: Option<u32>,
    /// See MachineFunction::string_number_after_rodata.
    pub string_number_after_rodata: Option<(u32, u32)>,
    /// The `@N` names of those NEW strings, in front-of-block order. The writer emits a
    /// LOCAL object symbol for each at the FRONT of this function's `@N` block (its bytes
    /// and section/offset come from the matching `.sdata`/`.data` data object), so the
    /// string symbol interleaves per-function with the constants/unwind entries the way
    /// mwcc lays them out — not grouped in the data run.
    pub string_names: Vec<String>,
    /// A dense `switch`'s jump table. The writer materializes it as an anonymous
    /// `@N` object in `.data`, fills the per-entry `ADDR32` relocations to this
    /// function, and resolves this function's `JumpTable` `.text` relocations.
    pub jump_tables: Vec<JumpTable>,
    /// An anonymous `.rodata` blob (`@N` via ADDR16_HA/LO): raw bytes plus the
    /// blob's offset past the function's running `@N` counter (numbered BEFORE
    /// the pool constants — measured on __strtold: table @26, pool @147).
    pub anonymous_rodata: Vec<(Vec<u8>, i32)>,
    /// Callees emitting LOCAL UND symbols in the explicit extern run.
    pub local_undefined_callees: Vec<String>,
    /// The names this function references (globals/callees) in mwcc's symbol-table
    /// order — an AST traversal, not `.text` reference order. The writer assigns
    /// this function's external/global symbols in this order, with a relocation-
    /// order fallback for anything not listed.
    pub symbol_order: Vec<String>,
    /// Resolve this analysis transaction's defined-data references before
    /// locally defined function targets under deferred code generation.
    pub defined_data_precedes_defined_functions: bool,
    /// Function-designator subset of `symbol_order`.
    pub referenced_function_symbols: Vec<String>,
    /// Callees this function references that were IMPLICITLY declared (K&R first-use, no
    /// prototype). mwcc creates their symbols at the call site inside the body, so the
    /// writer emits them AFTER this function's own symbol rather than before it.
    pub implicit_external_callees: Vec<String>,
    /// Implicit callees created before this function's referenced data symbols.
    pub early_implicit_external_callees: Vec<String>,
}

/// A dense `switch`'s jump table — one `.text` body offset per index, plus how far
/// the table's anonymous `@N` symbol sits past the function's running counter.
pub struct JumpTable {
    pub entries: Vec<u32>,
    pub anonymous_offset: u32,
}

/// What a `.text` relocation points at.
pub enum RelocationTarget {
    /// The i-th of the function's jump tables.
    JumpTableAt(usize),
    /// An external symbol defined elsewhere (a global or callee).
    External(String),
    /// An external symbol plus a byte ADDEND (an SDA21 load into a pooled string).
    ExternalWithAddend(String, i32),
    /// An entry in this object's constant pool, by index.
    Constant(usize),
    /// A constant-pool entry plus a byte ADDEND (the second word of an 8-byte image).
    ConstantWithAddend(usize, i32),
    /// This function's own jump table (the anonymous `@N` object in `.data`).
    JumpTable,
    /// The i-th of this function's `.rodata` blobs.
    AnonymousRodataAt(usize),
    /// This function's anonymous `.rodata` blob (`FunctionObject::anonymous_rodata`).
    AnonymousRodata,
}

/// A `.text` relocation: a byte offset, the ELF relocation type, and its target.
pub struct TextRelocation {
    pub offset: u32,
    pub elf_type: u32,
    pub target: RelocationTarget,
}

/// A compiler-managed read-only constant: its big-endian bit pattern, byte
/// width, and section-placement policy.
pub struct Sdata2Constant {
    pub bits: u64,
    pub byte_width: u8,
    /// Numbered at the function's static-local slot (`counter - 1`), not the
    /// pool block (an initialized auto array's pooled word image).
    pub static_slot: bool,
    /// The symbol leads the owning static function's FUNC symbol.
    pub image: bool,
    /// A fresh slot even when an equal constant exists (twin zero doubles).
    pub force_new: bool,
    /// Place the constant in `.rodata` instead of `.sdata2`. Absolute
    /// ADDR16_HA/LO references require this when read-only small data is off.
    pub force_full_data_section: bool,
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
