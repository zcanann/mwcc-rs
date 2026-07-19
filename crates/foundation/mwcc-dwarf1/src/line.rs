use crate::{push_relocatable_u32, push_u16, push_u32, Address, EncodedSection};

/// One DWARF 1 source-statement record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRecord {
    pub line: u32,
    pub column: u16,
    /// Byte offset from the table's relocatable base address.
    pub address_delta: u32,
}

/// A complete CodeWarrior `.line` section for one translation unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineTable {
    pub base_address: Address,
    pub records: Vec<LineRecord>,
}

impl LineTable {
    pub fn encode(&self) -> EncodedSection {
        let byte_len = 8_u32 + self.records.len() as u32 * 10;
        let mut section = EncodedSection {
            bytes: Vec::with_capacity(byte_len as usize),
            relocations: Vec::with_capacity(1),
        };

        push_u32(&mut section.bytes, byte_len);
        push_relocatable_u32(&mut section, &self.base_address);
        for record in &self.records {
            push_u32(&mut section.bytes, record.line);
            push_u16(&mut section.bytes, record.column);
            push_u32(&mut section.bytes, record.address_delta);
        }
        section
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Relocation, RelocationTarget};

    #[test]
    fn encodes_legacy_codewarrior_line_table() {
        let encoded = LineTable {
            base_address: Address::external(".text"),
            records: vec![
                LineRecord {
                    line: 7,
                    column: 0,
                    address_delta: 0,
                },
                LineRecord {
                    line: 0,
                    column: 0,
                    address_delta: 8,
                },
            ],
        }
        .encode();

        assert_eq!(
            encoded.bytes,
            [
                0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08,
            ]
        );
        assert_eq!(
            encoded.relocations,
            [Relocation {
                offset: 4,
                target: RelocationTarget::External(".text".into()),
                addend: 0,
            }]
        );
    }

    #[test]
    fn encodes_fragmented_codewarrior_line_table() {
        let encoded = LineTable {
            base_address: Address::external(".text"),
            records: vec![
                LineRecord {
                    line: 7,
                    column: 0,
                    address_delta: 0,
                },
                LineRecord {
                    line: 8,
                    column: 0,
                    address_delta: 4,
                },
                LineRecord {
                    line: 0,
                    column: 0,
                    address_delta: 8,
                },
            ],
        }
        .encode();

        assert_eq!(encoded.bytes.len(), 0x26);
        assert_eq!(&encoded.bytes[0..4], &[0x00, 0x00, 0x00, 0x26]);
        assert_eq!(
            &encoded.bytes[8..],
            &[
                0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08,
            ]
        );
    }
}
