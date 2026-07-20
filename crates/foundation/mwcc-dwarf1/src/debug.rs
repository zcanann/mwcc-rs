use crate::{push_relocatable_u32, push_u16, push_u32, Address, EncodedSection};

/// Stable handle used by inter-DIE references before final byte offsets exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DebugEntryId(pub u32);

/// DWARF 1 tag values used by CodeWarrior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Tag {
    FormalParameter = 0x0005,
    GlobalSubroutine = 0x0006,
    GlobalVariable = 0x0007,
    LocalVariable = 0x000c,
    CompileUnit = 0x0011,
}

/// The high bits of a DWARF 1 attribute code. The low nibble is its form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum AttributeName {
    Sibling = 0x0010,
    Location = 0x0020,
    Name = 0x0030,
    FundamentalType = 0x0050,
    StatementList = 0x0100,
    LowPc = 0x0110,
    HighPc = 0x0120,
    Language = 0x0130,
    Producer = 0x0250,
}

/// DWARF 1 fundamental type identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum FundamentalType {
    SignedInteger = 0x0007,
}

/// An attribute value. Its variant determines the DWARF 1 form nibble.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    Address(Address),
    Reference(DebugEntryId),
    Block2(Vec<u8>),
    RelocatableBlock2(Block),
    Block4(Vec<u8>),
    Data2(u16),
    Data4(u32),
    /// A relocatable four-byte value encoded with the DATA4 form. CodeWarrior
    /// uses this for `AT_stmt_list`, even though the payload names `.line`.
    Data4Address(Address),
    Data8(u64),
    String(String),
}

impl AttributeValue {
    fn form(&self) -> u16 {
        match self {
            Self::Address(_) => 0x1,
            Self::Reference(_) => 0x2,
            Self::Block2(_) | Self::RelocatableBlock2(_) => 0x3,
            Self::Block4(_) => 0x4,
            Self::Data2(_) => 0x5,
            Self::Data4(_) | Self::Data4Address(_) => 0x6,
            Self::Data8(_) => 0x7,
            Self::String(_) => 0x8,
        }
    }
}

/// A location-expression block with relocatable four-byte operands. Relocation
/// offsets are relative to the first byte after the block's length field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub bytes: Vec<u8>,
    pub relocations: Vec<BlockRelocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRelocation {
    pub offset: u32,
    pub address: Address,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: AttributeName,
    pub value: AttributeValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugEntry {
    pub id: DebugEntryId,
    pub tag: Tag,
    pub attributes: Vec<Attribute>,
}

/// An ordered DWARF 1 DIE stream. Tree terminators are represented explicitly
/// as their raw byte lengths because CodeWarrior uses several distinct terminal
/// records whose exact semantic distinction still needs broader measurements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugInfo {
    pub entries: Vec<DebugEntry>,
    pub terminal_records: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedDebugInfo {
    pub section: EncodedSection,
    pub entry_offsets: Vec<(DebugEntryId, u32)>,
}

impl DebugInfo {
    pub fn encode(&self) -> EncodedSection {
        self.encode_with_offsets().section
    }

    pub fn encode_with_offsets(&self) -> EncodedDebugInfo {
        let mut section = EncodedSection::default();
        let mut entry_offsets = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let start = section.bytes.len();
            entry_offsets.push((entry.id, start as u32));
            push_u32(&mut section.bytes, 0);
            push_u16(&mut section.bytes, entry.tag as u16);
            for attribute in &entry.attributes {
                push_u16(
                    &mut section.bytes,
                    attribute.name as u16 | attribute.value.form(),
                );
                encode_value(&mut section, &attribute.value);
            }
            let byte_len = (section.bytes.len() - start) as u32;
            section.bytes[start..start + 4].copy_from_slice(&byte_len.to_be_bytes());
        }
        for record in &self.terminal_records {
            section.bytes.extend_from_slice(record);
        }
        EncodedDebugInfo {
            section,
            entry_offsets,
        }
    }
}

fn encode_value(section: &mut EncodedSection, value: &AttributeValue) {
    match value {
        AttributeValue::Address(address) => push_relocatable_u32(section, address),
        AttributeValue::Reference(id) => {
            push_relocatable_u32(section, &Address::debug_entry(*id));
        }
        AttributeValue::Block2(bytes) => {
            push_u16(&mut section.bytes, bytes.len() as u16);
            section.bytes.extend_from_slice(bytes);
        }
        AttributeValue::RelocatableBlock2(block) => {
            push_u16(&mut section.bytes, block.bytes.len() as u16);
            let block_start = section.bytes.len() as u32;
            section.bytes.extend_from_slice(&block.bytes);
            for relocation in &block.relocations {
                section.relocations.push(crate::Relocation {
                    offset: block_start + relocation.offset,
                    target: relocation.address.target.clone(),
                    addend: relocation.address.addend,
                });
            }
        }
        AttributeValue::Block4(bytes) => {
            push_u32(&mut section.bytes, bytes.len() as u32);
            section.bytes.extend_from_slice(bytes);
        }
        AttributeValue::Data2(value) => push_u16(&mut section.bytes, *value),
        AttributeValue::Data4(value) => push_u32(&mut section.bytes, *value),
        AttributeValue::Data4Address(address) => push_relocatable_u32(section, address),
        AttributeValue::Data8(value) => section.bytes.extend_from_slice(&value.to_be_bytes()),
        AttributeValue::String(value) => {
            section.bytes.extend_from_slice(value.as_bytes());
            section.bytes.push(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Relocation, RelocationTarget};

    #[test]
    fn encodes_measured_codewarrior_compile_unit() {
        let end = DebugEntryId(1);
        let encoded = DebugInfo {
            entries: vec![DebugEntry {
                id: DebugEntryId(0),
                tag: Tag::CompileUnit,
                attributes: vec![
                    Attribute {
                        name: AttributeName::Sibling,
                        value: AttributeValue::Reference(end),
                    },
                    Attribute {
                        name: AttributeName::Producer,
                        value: AttributeValue::String("MW EABI PPC C-Compiler".into()),
                    },
                    Attribute {
                        name: AttributeName::Name,
                        value: AttributeValue::String("1236_debug_info_basic.c".into()),
                    },
                    Attribute {
                        name: AttributeName::Language,
                        value: AttributeValue::Data4(1),
                    },
                    Attribute {
                        name: AttributeName::LowPc,
                        value: AttributeValue::Address(Address::external(".text")),
                    },
                    Attribute {
                        name: AttributeName::HighPc,
                        value: AttributeValue::Address(Address::external_with_addend(".text", 8)),
                    },
                    Attribute {
                        name: AttributeName::StatementList,
                        value: AttributeValue::Data4Address(Address::external(".line")),
                    },
                ],
            }],
            terminal_records: Vec::new(),
        }
        .encode();

        assert_eq!(encoded.bytes.len(), 0x57);
        assert_eq!(
            &encoded.bytes,
            &[
                0x00, 0x00, 0x00, 0x57, 0x00, 0x11, 0x00, 0x12, 0x00, 0x00,
                0x00, 0x00, 0x02, 0x58, 0x4d, 0x57, 0x20, 0x45, 0x41, 0x42,
                0x49, 0x20, 0x50, 0x50, 0x43, 0x20, 0x43, 0x2d, 0x43, 0x6f,
                0x6d, 0x70, 0x69, 0x6c, 0x65, 0x72, 0x00, 0x00, 0x38, 0x31,
                0x32, 0x33, 0x36, 0x5f, 0x64, 0x65, 0x62, 0x75, 0x67, 0x5f,
                0x69, 0x6e, 0x66, 0x6f, 0x5f, 0x62, 0x61, 0x73, 0x69, 0x63,
                0x2e, 0x63, 0x00, 0x01, 0x36, 0x00, 0x00, 0x00, 0x01, 0x01,
                0x11, 0x00, 0x00, 0x00, 0x00, 0x01, 0x21, 0x00, 0x00, 0x00,
                0x00, 0x01, 0x06, 0x00, 0x00, 0x00, 0x00,
            ]
        );
        assert_eq!(
            encoded.relocations,
            [
                Relocation {
                    offset: 8,
                    target: RelocationTarget::DebugEntry(end),
                    addend: 0,
                },
                Relocation {
                    offset: 0x47,
                    target: RelocationTarget::External(".text".into()),
                    addend: 0,
                },
                Relocation {
                    offset: 0x4d,
                    target: RelocationTarget::External(".text".into()),
                    addend: 8,
                },
                Relocation {
                    offset: 0x53,
                    target: RelocationTarget::External(".line".into()),
                    addend: 0,
                },
            ]
        );
    }
}
