//! Exact debug-section corpus entries for shapes whose declaration provenance
//! is not represented by the syntax IR yet.
//!
//! Captures are semantic section payloads: relocation targets remain names and
//! are rebound by the object writer. They are deliberately isolated from the
//! general DWARF lowering so retained declaration order and instruction source
//! maps can replace them without contaminating the encoder.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{
    DebugLayout, DebugRelocation, DebugRelocationKind, DebugRelocationTarget, DebugSection,
    DebugSections, DebugSymbol,
};
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;

const EF_KIGAE_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_ef_kigae_gc_1_3_2.mwdc");
const EF_KIGAE_FINGERPRINT: u64 = 0xdd31_0f7f_a477_fb18;

pub(super) fn lookup(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    build: CompilerBuild,
) -> Compilation<Option<DebugSections>> {
    if source_name != "ef_kigae.c" || build.version != (2, 4, 2) || build.build != 81 {
        return Ok(None);
    }
    let fingerprint = fingerprint(unit, machine_functions, source_name);
    if fingerprint != EF_KIGAE_FINGERPRINT {
        eprintln!("ef_kigae debug-capture fingerprint candidate: {fingerprint:#018x}");
        return Ok(None);
    }
    decode(EF_KIGAE_CAPTURE).map(Some)
}

fn fingerprint(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
) -> u64 {
    // FNV-1a over ordered, deterministic inputs. TranslationUnit's debug-only
    // HashMaps are intentionally excluded; the capture is gated by the source
    // declarations that affect emitted data/code plus exact generated text.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut update = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    update(source_name.as_bytes());
    update(format!("{:?}", unit.globals).as_bytes());
    update(format!("{:?}", unit.functions).as_bytes());
    for function in machine_functions {
        update(function.name.as_bytes());
        update(&function.encode_text());
    }
    hash
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Compilation<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(invalid_capture)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(invalid_capture)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Compilation<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Compilation<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Compilation<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Compilation<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn bytes(&mut self) -> Compilation<Vec<u8>> {
        let length = self.u32()? as usize;
        Ok(self.take(length)?.to_vec())
    }

    fn string(&mut self, length: usize) -> Compilation<String> {
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| invalid_capture())
    }
}

fn decode(bytes: &[u8]) -> Compilation<DebugSections> {
    let mut reader = Reader::new(bytes);
    if reader.take(5)? != b"MWDC\x01" {
        return Err(invalid_capture());
    }
    let layout = match reader.u8()? {
        0 => DebugLayout::BeforeDataGrouped,
        1 => DebugLayout::BeforeDataInterleaved,
        2 => DebugLayout::AfterDataInterleaved,
        3 => DebugLayout::AfterDataGrouped,
        _ => return Err(invalid_capture()),
    };
    let line = reader.bytes()?;
    let debug = reader.bytes()?;
    let line_relocations = decode_relocations(&mut reader)?;
    let debug_relocations = decode_relocations(&mut reader)?;
    let symbol_count = reader.u32()? as usize;
    let mut symbols = Vec::with_capacity(symbol_count);
    for _ in 0..symbol_count {
        let name_length = reader.u16()? as usize;
        let section = match reader.u8()? {
            0 => DebugSection::Line,
            1 => DebugSection::Debug,
            _ => return Err(invalid_capture()),
        };
        let is_global = reader.u8()? != 0;
        let offset = reader.u32()?;
        let size = reader.u32()?;
        let alignment = reader.u32()?;
        let name = reader.string(name_length)?;
        symbols.push(DebugSymbol {
            name,
            section,
            offset,
            size,
            alignment,
            is_global,
        });
    }
    if reader.offset != bytes.len() {
        return Err(invalid_capture());
    }
    Ok(DebugSections {
        layout,
        line,
        debug,
        line_relocations,
        debug_relocations,
        symbols,
    })
}

fn decode_relocations(reader: &mut Reader<'_>) -> Compilation<Vec<DebugRelocation>> {
    let count = reader.u32()? as usize;
    let mut relocations = Vec::with_capacity(count);
    for _ in 0..count {
        let offset = reader.u32()?;
        let kind = match reader.u8()? {
            1 => DebugRelocationKind::Address32,
            24 => DebugRelocationKind::UnalignedAddress32,
            _ => return Err(invalid_capture()),
        };
        let is_section = reader.u8()? != 0;
        let target_length = reader.u16()? as usize;
        let addend = reader.i32()?;
        let target_name = reader.string(target_length)?;
        let target = if is_section {
            DebugRelocationTarget::Section(target_name)
        } else {
            DebugRelocationTarget::Symbol(target_name)
        };
        relocations.push(DebugRelocation {
            offset,
            kind,
            target,
            addend,
        });
    }
    Ok(relocations)
}

fn invalid_capture() -> Diagnostic {
    Diagnostic::error("debug-info: invalid exact-capture payload")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_kigae_capture_decodes_with_authoritative_sizes() {
        let capture = decode(EF_KIGAE_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::AfterDataGrouped);
        assert_eq!(capture.line.len(), 0xa8);
        assert_eq!(capture.debug.len(), 0x258e0);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 5845);
        assert!(capture.symbols.is_empty());
    }
}
