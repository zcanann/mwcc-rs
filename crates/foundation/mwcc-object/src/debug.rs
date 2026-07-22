//! Object-container vocabulary for CodeWarrior DWARF 1 sections.
//!
//! DWARF encoding lives in `mwcc-dwarf1`; this module describes only how those
//! bytes participate in ELF section, symbol, and relocation tables.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugSection {
    Line,
    Debug,
}

impl DebugSection {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Line => ".line",
            Self::Debug => ".debug",
        }
    }
}

/// Measured generations of CodeWarrior's debug-aware ELF layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugLayout {
    /// Debug content precedes ordinary data; relocation and metadata sections
    /// remain grouped at the end (GC/1.2.5, GC/1.3, GC/2.x).
    BeforeDataGrouped,
    /// Each debug content section is immediately followed by its relocation;
    /// `.comment` precedes the symbol tables (GC/1.3.2).
    BeforeDataInterleaved,
    /// Full-size `.rodata`/`.data`/`.bss` precede debug content, while small
    /// `.sdata`/`.sbss`/`.sdata2` follows it (legacy data-only units).
    BetweenFullAndSmallDataGrouped,
    /// Ordinary data precedes debug content; each debug section is immediately
    /// followed by its relocation.
    AfterDataInterleaved,
    /// Ordinary data precedes debug content while relocations remain grouped
    /// and `.comment` stays last (measured GC/3.0a3 and Wii/1.0 Runtime objects).
    AfterDataGrouped,
}

impl DebugLayout {
    pub(crate) fn before_data(self) -> bool {
        matches!(self, Self::BeforeDataGrouped | Self::BeforeDataInterleaved)
    }

    pub(crate) fn between_full_and_small_data(self) -> bool {
        self == Self::BetweenFullAndSmallDataGrouped
    }

    pub(crate) fn interleaved_relocations(self) -> bool {
        matches!(
            self,
            Self::BeforeDataInterleaved | Self::AfterDataInterleaved
        )
    }

    pub(crate) fn comment_before_symbols(self) -> bool {
        self.interleaved_relocations()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugRelocationKind {
    Address32,
    UnalignedAddress32,
}

impl DebugRelocationKind {
    pub(crate) fn elf_type(self) -> u32 {
        match self {
            Self::Address32 => 1,
            Self::UnalignedAddress32 => 24,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugRelocationTarget {
    /// A section symbol such as `.text`, `.line`, or `.debug`.
    Section(String),
    /// A named local/global symbol, including modern `.dwarf.*` fragments.
    Symbol(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugRelocation {
    pub offset: u32,
    pub kind: DebugRelocationKind,
    pub target: DebugRelocationTarget,
    pub addend: i32,
}

/// A CodeWarrior fragment symbol inside `.line` or `.debug`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugSymbolBinding {
    Local,
    Global,
    Weak,
}

/// Creation point of a local fragmented-DWARF symbol in MWCC's local symbol
/// stream. Most captured/legacy symbols lead ordinary anonymous objects; modern
/// function fragments can close only after that function's constants/unwind
/// entries have been created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugSymbolPlacement {
    Early,
    AfterFunctionLocals(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugSymbol {
    pub name: String,
    pub section: DebugSection,
    pub offset: u32,
    pub size: u32,
    pub alignment: u32,
    /// Exact per-symbol attribute word stored in MWCC's `.comment` table.
    pub comment_flags: u32,
    pub binding: DebugSymbolBinding,
    pub placement: DebugSymbolPlacement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugSections {
    pub layout: DebugLayout,
    /// Fragmented debug generations can redistribute ordinal bookkeeping
    /// across a framed function boundary. The object planner consumes this
    /// override while assigning unwind symbols; monolithic formats leave it
    /// unset and use the build's ordinary post-function convention.
    pub post_framed_function_anonymous_bump_override: Option<u8>,
    pub line: Vec<u8>,
    pub debug: Vec<u8>,
    pub line_relocations: Vec<DebugRelocation>,
    pub debug_relocations: Vec<DebugRelocation>,
    pub symbols: Vec<DebugSymbol>,
}
