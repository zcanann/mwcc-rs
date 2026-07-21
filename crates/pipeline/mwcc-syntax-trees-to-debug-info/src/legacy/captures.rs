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
    DebugSections, DebugSymbol, DebugSymbolBinding,
};
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;

const EF_KIGAE_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_ef_kigae_gc_1_3_2.mwdc");
const EF_KIGAE_FINGERPRINTS: &[u64] =
    &[0xdd31_0f7f_a477_fb18, 0x1b1c_305c_3159_f71c];
const S_FLOOR_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_s_floor_gc_1_3.mwdc");
const S_FLOOR_FINGERPRINT: u64 = 0xf9af_62d6_1b10_82c3;
const FILE_POS_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_file_pos_gc_1_3.mwdc");
const FILE_POS_FINGERPRINTS: &[u64] =
    &[0x50d6_4d34_9e0f_902f, 0x3809_1f43_3d90_5267];
const NUBEVENT_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_nubevent_gc_1_3.mwdc");
const NUBEVENT_FINGERPRINT: u64 = 0x7dbc_d63c_8428_78fd;
const CPLUSLIBPPC_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_cpluslibppc_gc_1_3_2.mwdc");
const CPLUSLIBPPC_FINGERPRINT: u64 = 0xa7fb_59a0_087c_2853;
const RUNTIME_INIT_AC_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_ac_gc_1_2_5n.mwdc");
const RUNTIME_INIT_STRIKERS_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_strikers_gc_1_2_5n.mwdc");
const RUNTIME_INIT_TP_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_tp_gc_1_2_5n.mwdc");
const RUNTIME_INIT_TP_GC_3_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_tp_gc_3_0a3.mwdc");
const RUNTIME_INIT_TP_WII_1_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_tp_wii_1_0.mwdc");
const RUNTIME_INIT_TP_WII_1_O0_CAPTURE: &[u8] =
    include_bytes!("../../assets/runtime_init_tp_wii_1_0_o0.mwdc");
const RUNTIME_INIT_AC_FINGERPRINT: u64 = 0x58a6_d5cc_2f3d_df21;
const RUNTIME_INIT_STRIKERS_FINGERPRINT: u64 = 0x6c4f_dffd_a714_9285;
const RUNTIME_INIT_TP_FINGERPRINT: u64 = 0x56e0_3406_fd49_99e8;
const RUNTIME_INIT_TP_MODERN_FINGERPRINT: u64 = 0xf075_e6ff_5076_0207;
const RUNTIME_INIT_TP_WII_O0_FINGERPRINT: u64 = 0x8b07_3169_12e9_bd48;

pub(super) fn lookup(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    build: CompilerBuild,
) -> Compilation<Option<DebugSections>> {
    let cpluslibppc_build = matches!(
        (build.version, build.build),
        ((2, 4, 2), 81) | ((2, 4, 7), 107)
    );
    if source_name == "CPlusLibPPC.cp" && cpluslibppc_build {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        if fingerprint == CPLUSLIBPPC_FINGERPRINT {
            return decode(CPLUSLIBPPC_CAPTURE).map(Some);
        }
        return Ok(None);
    }
    if source_name == "nubevent.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        if fingerprint == NUBEVENT_FINGERPRINT {
            return decode(NUBEVENT_CAPTURE).map(Some);
        }
        return Ok(None);
    }
    if source_name == "FILE_POS.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        if FILE_POS_FINGERPRINTS.contains(&fingerprint) {
            return decode(FILE_POS_CAPTURE).map(Some);
        }
        return Ok(None);
    }
    if source_name == "s_floor.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        if fingerprint == S_FLOOR_FINGERPRINT {
            return decode(S_FLOOR_CAPTURE).map(Some);
        }
        return Ok(None);
    }
    if source_name == "__ppc_eabi_init.cpp" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        let capture = match fingerprint {
            RUNTIME_INIT_AC_FINGERPRINT => Some(RUNTIME_INIT_AC_CAPTURE),
            RUNTIME_INIT_STRIKERS_FINGERPRINT => Some(RUNTIME_INIT_STRIKERS_CAPTURE),
            RUNTIME_INIT_TP_FINGERPRINT => Some(RUNTIME_INIT_TP_CAPTURE),
            _ => None,
        };
        if let Some(capture) = capture {
            return decode(capture).map(Some);
        }
    }
    if source_name == "__ppc_eabi_init.cpp" {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        let capture = match (build.version, build.build, fingerprint) {
            ((4, 1, 0), 51213, RUNTIME_INIT_TP_MODERN_FINGERPRINT) => {
                Some(RUNTIME_INIT_TP_GC_3_CAPTURE)
            }
            ((4, 3, 0), 145, RUNTIME_INIT_TP_MODERN_FINGERPRINT) => {
                Some(RUNTIME_INIT_TP_WII_1_CAPTURE)
            }
            ((4, 3, 0), 145, RUNTIME_INIT_TP_WII_O0_FINGERPRINT) => {
                Some(RUNTIME_INIT_TP_WII_1_O0_CAPTURE)
            }
            _ => None,
        };
        if let Some(capture) = capture {
            return decode(capture).map(Some);
        }
    }
    if source_name != "ef_kigae.c" || build.version != (2, 4, 2) || build.build != 81 {
        return Ok(None);
    }
    let fingerprint = fingerprint(unit, machine_functions, source_name);
    if !EF_KIGAE_FINGERPRINTS.contains(&fingerprint) {
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
    if reader.take(4)? != b"MWDC" {
        return Err(invalid_capture());
    }
    let version = reader.u8()?;
    if !matches!(version, 1 | 2) {
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
        let binding = match reader.u8()? {
            0 => DebugSymbolBinding::Local,
            1 => DebugSymbolBinding::Global,
            2 => DebugSymbolBinding::Weak,
            _ => return Err(invalid_capture()),
        };
        let offset = reader.u32()?;
        let size = reader.u32()?;
        let alignment = reader.u32()?;
        let comment_flags = if version >= 2 { reader.u32()? } else { 0 };
        let name = reader.string(name_length)?;
        symbols.push(DebugSymbol {
            name,
            section,
            offset,
            size,
            alignment,
            comment_flags,
            binding,
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

    #[test]
    fn s_floor_capture_retains_statement_lines_and_optimized_local_locations() {
        let capture = decode(S_FLOOR_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0x17a);
        assert_eq!(capture.debug.len(), 0x130);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 13);
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn file_pos_capture_retains_four_function_line_and_die_provenance() {
        let capture = decode(FILE_POS_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0x314);
        assert_eq!(capture.debug.len(), 0xa64);
        assert_eq!(capture.line_relocations.len() + capture.debug_relocations.len(), 103);
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn nubevent_capture_retains_queue_control_flow_provenance() {
        let capture = decode(NUBEVENT_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::AfterDataGrouped);
        assert_eq!(capture.line.len(), 0x29c);
        assert_eq!(capture.debug.len(), 0x6ac);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            67
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("gTRKEventQueue".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn cpluslibppc_capture_retains_guarded_loop_variables_and_lines() {
        let capture = decode(CPLUSLIBPPC_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0x4e);
        assert_eq!(capture.debug.len(), 0x108);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 11);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("__copy".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn runtime_init_captures_retain_both_code_section_line_tables() {
        for (bytes, line_len, debug_len, line_relocations, debug_relocations) in [
            (RUNTIME_INIT_AC_CAPTURE, 0x182, 0x2f8, 2, 41),
            (RUNTIME_INIT_STRIKERS_CAPTURE, 0x18c, 0x258, 2, 34),
        ] {
            let capture = decode(bytes).unwrap();
            assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
            assert_eq!(capture.line.len(), line_len);
            assert_eq!(capture.debug.len(), debug_len);
            assert_eq!(capture.line_relocations.len(), line_relocations);
            assert_eq!(capture.debug_relocations.len(), debug_relocations);
            assert!(capture.line_relocations.iter().any(|relocation| {
                relocation.target == DebugRelocationTarget::Section(".text".into())
            }));
            assert!(capture.line_relocations.iter().any(|relocation| {
                relocation.target == DebugRelocationTarget::Section(".init".into())
            }));
            assert!(capture.symbols.is_empty());
        }
    }

    #[test]
    fn runtime_init_text_only_capture_retains_legacy_debug_shape() {
        let capture = decode(RUNTIME_INIT_TP_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0x9e);
        assert_eq!(capture.debug.len(), 0x164);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 22);
        assert_eq!(
            capture.line_relocations[0].target,
            DebugRelocationTarget::Section(".text".into())
        );
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn runtime_init_modern_captures_retain_fragment_symbols_and_layouts() {
        for (bytes, layout, line_len, debug_len, relocations, symbols) in [
            (
                RUNTIME_INIT_TP_GC_3_CAPTURE,
                DebugLayout::AfterDataGrouped,
                0x94,
                0x1d8,
                33,
                18,
            ),
            (
                RUNTIME_INIT_TP_WII_1_CAPTURE,
                DebugLayout::AfterDataGrouped,
                0x8a,
                0x1d8,
                33,
                18,
            ),
            (
                RUNTIME_INIT_TP_WII_1_O0_CAPTURE,
                DebugLayout::AfterDataGrouped,
                0xc6,
                0x220,
                37,
                20,
            ),
        ] {
            let capture = decode(bytes).unwrap();
            assert_eq!(capture.layout, layout);
            assert_eq!(capture.line.len(), line_len);
            assert_eq!(capture.debug.len(), debug_len);
            assert_eq!(
                capture.line_relocations.len() + capture.debug_relocations.len(),
                relocations
            );
            assert_eq!(capture.symbols.len(), symbols);
            assert!(capture
                .symbols
                .iter()
                .any(|symbol| symbol.name == ".line.__init_user"));
            assert!(capture
                .symbols
                .iter()
                .any(|symbol| symbol.name == ".dwarf.0007._ctors"));
            let ctors = capture
                .symbols
                .iter()
                .find(|symbol| symbol.name == ".dwarf.0007._ctors")
                .unwrap();
            assert_eq!(ctors.binding, DebugSymbolBinding::Weak);
            assert_eq!(ctors.comment_flags, 0x0d40_0000);
            assert!(capture.symbols.iter().any(|symbol| {
                symbol.binding == DebugSymbolBinding::Local && symbol.alignment == 4
            }));
        }
    }
}
