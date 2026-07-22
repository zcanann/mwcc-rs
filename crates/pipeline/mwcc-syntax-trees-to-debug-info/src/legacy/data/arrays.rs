//! DWARF-1 subscript descriptors for scalar and aggregate arrays.

use mwcc_dwarf1::{Address, Block, BlockRelocation, DebugEntryId, FundamentalType};

pub(super) fn fundamental_subscript_data(
    length: u16,
    fundamental: FundamentalType,
) -> Vec<u8> {
    let mut bytes = bounds(length);
    bytes.extend_from_slice(&[8, 0, 0x55]);
    bytes.extend_from_slice(&(fundamental as u16).to_be_bytes());
    bytes
}

pub(super) fn aggregate_subscript_data(length: u16, aggregate: DebugEntryId) -> Block {
    let mut bytes = bounds(length);
    bytes.extend_from_slice(&[8, 0, 0x72]);
    let relocation_offset = bytes.len() as u32;
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    Block {
        bytes,
        relocations: vec![BlockRelocation {
            offset: relocation_offset,
            address: Address::debug_entry(aggregate),
        }],
    }
}

fn bounds(length: u16) -> Vec<u8> {
    let mut bytes = vec![0, 0, 10];
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&u32::from(length - 1).to_be_bytes());
    bytes
}
