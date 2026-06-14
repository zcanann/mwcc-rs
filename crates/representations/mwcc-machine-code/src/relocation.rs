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

/// A relocation against `.text`, located by the instruction it patches (byte
/// offset = index * 4) and naming the referenced symbol.
#[derive(Debug, Clone)]
pub struct Relocation {
    pub instruction_index: usize,
    pub kind: RelocationKind,
    pub symbol: String,
}
