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
    /// `R_PPC_ADDR16_HA` (6) — the high-adjusted 16 bits of an absolute address,
    /// patched into the immediate of a `lis` (the `-sdata 0` addressing mode).
    Addr16Ha,
    /// `R_PPC_ADDR16_LO` (4) — the low 16 bits of an absolute address, patched
    /// into the immediate/displacement of the following `addi`/load/store.
    Addr16Lo,
}

impl RelocationKind {
    /// The ELF relocation type number.
    pub fn elf_type(self) -> u32 {
        match self {
            RelocationKind::EmbSda21 => 109,
            RelocationKind::Rel24 => 10,
            RelocationKind::Addr16Ha => 6,
            RelocationKind::Addr16Lo => 4,
        }
    }

    /// Byte offset of the patched field within the 4-byte instruction. The 16-bit
    /// ADDR16 fields live in the instruction's low halfword (offset 2, big-endian);
    /// SDA21 and REL24 are described from the instruction start (offset 0).
    pub fn field_offset(self) -> u32 {
        match self {
            RelocationKind::Addr16Ha | RelocationKind::Addr16Lo => 2,
            RelocationKind::EmbSda21 | RelocationKind::Rel24 => 0,
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
    /// This function's own jump table — the anonymous `@N` object the writer
    /// materializes in `.data` for a dense `switch` (its `lis`/`addi` address load).
    JumpTable,
    /// This function's anonymous `.rodata` blob (`MachineFunction::anonymous_rodata`).
    AnonymousRodata,
}

/// A relocation against `.text`, located by the instruction it patches (byte
/// offset = index * 4) and naming the referenced target.
#[derive(Debug, Clone)]
pub struct Relocation {
    pub instruction_index: usize,
    pub kind: RelocationKind,
    pub target: RelocationTarget,
}
