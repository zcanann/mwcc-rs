//! DWARF 1 byte encoding used by Metrowerks PowerPC compilers.
//!
//! This crate deliberately stops at relocatable section contents. It knows the
//! DWARF record layout, but not ELF section indexes, symbol-table ordering, or
//! a particular compiler build's synthetic symbol names. Those are object
//! format concerns and belong to the object writer.

mod debug;
mod line;

pub use debug::{
    Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry, DebugEntryId,
    DebugInfo, DebugRecord, EncodedDebugInfo, FundamentalType, Tag,
};
pub use line::{LineRecord, LineTable};

/// Relocatable bytes for one DWARF section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncodedSection {
    pub bytes: Vec<u8>,
    pub relocations: Vec<Relocation>,
}

/// The object-level entity named by a DWARF address or reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelocationTarget {
    /// A caller-owned ELF section or symbol.
    External(String),
    /// The beginning of a DIE in the `.debug` section.
    DebugEntry(DebugEntryId),
}

/// One four-byte field the object writer must relocate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relocation {
    pub offset: u32,
    pub target: RelocationTarget,
    pub addend: i32,
}

/// A symbolic address retained until ELF emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub target: RelocationTarget,
    pub addend: i32,
}

impl Address {
    pub fn external(name: impl Into<String>) -> Self {
        Self {
            target: RelocationTarget::External(name.into()),
            addend: 0,
        }
    }

    pub fn external_with_addend(name: impl Into<String>, addend: i32) -> Self {
        Self {
            target: RelocationTarget::External(name.into()),
            addend,
        }
    }

    pub fn debug_entry(id: DebugEntryId) -> Self {
        Self {
            target: RelocationTarget::DebugEntry(id),
            addend: 0,
        }
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_relocatable_u32(
    section: &mut EncodedSection,
    address: &Address,
) {
    section.relocations.push(Relocation {
        offset: section.bytes.len() as u32,
        target: address.target.clone(),
        addend: address.addend,
    });
    push_u32(&mut section.bytes, 0);
}
