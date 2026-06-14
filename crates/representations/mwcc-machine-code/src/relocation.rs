//! Relocations a function's `.text` needs: a reference from an instruction to a
//! symbol whose final value the linker supplies. The codegen records these as it
//! emits the referencing instruction; the object writer turns them into a
//! `.rela.text` section and the matching (often undefined) symbols.

/// The PowerPC/EABI relocation kinds we emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocationKind {
    /// `R_PPC_EMB_SDA21` (109) — small-data-area reference off r13/r2; patches an
    /// instruction's base register and 16-bit displacement (global access).
    EmbSda21,
    /// `R_PPC_REL24` (10) — the 24-bit branch displacement of a `bl` (a call).
    Rel24,
}

impl RelocationKind {
    /// The ELF relocation type number.
    pub fn elf_type(self) -> u32 {
        match self {
            RelocationKind::EmbSda21 => 109,
            RelocationKind::Rel24 => 10,
        }
    }
}

/// What a relocation points at: a named symbol defined elsewhere (a global or a
/// callee, emitted as an undefined external) or an entry in this function's own
/// constant pool (an anonymous `@N` object the writer materializes in `.sdata2`).
#[derive(Debug, Clone)]
pub enum RelocationTarget {
    External(String),
    Constant(usize),
}

/// A relocation against `.text`, located by the instruction it patches (byte
/// offset = index * 4) and naming the referenced target.
#[derive(Debug, Clone)]
pub struct Relocation {
    pub instruction_index: usize,
    pub kind: RelocationKind,
    pub target: RelocationTarget,
}
