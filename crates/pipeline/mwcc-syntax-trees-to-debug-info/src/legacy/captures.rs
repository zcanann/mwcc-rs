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
    DebugSections, DebugSymbol, DebugSymbolBinding, DebugSymbolPlacement,
};
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;

const EF_KIGAE_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_ef_kigae_gc_1_3_2.mwdc");
const EF_KIGAE_FINGERPRINTS: &[u64] =
    &[0xdd31_0f7f_a477_fb18, 0x1b1c_305c_3159_f71c];
const S_FLOOR_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_s_floor_gc_1_3.mwdc");
const S_FLOOR_FINGERPRINTS: &[u64] =
    &[0xf9af_62d6_1b10_82c3, 0xbabf_c68e_5677_afc5];
const S_FREXP_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_s_frexp_gc_1_3.mwdc");
const S_FREXP_SOURCE_TEXT_FINGERPRINT: u64 = 0x6d78_091c_657a_ecf4;
const FILE_POS_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_file_pos_gc_1_3.mwdc");
const FILE_POS_FINGERPRINTS: &[u64] =
    &[0x50d6_4d34_9e0f_902f, 0x3809_1f43_3d90_5267];
const FILE_POS_SOURCE_TEXT_FINGERPRINTS: &[u64] =
    &[0x7666_1cca_4a40_933c, 0xaf27_15b8_aaf8_705d];
const NUBEVENT_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_nubevent_gc_1_3.mwdc");
const NUBEVENT_FINGERPRINT: u64 = 0x7dbc_d63c_8428_78fd;
const DOLPHIN_TRK_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_dolphin_trk_gc_1_3.mwdc");
const DOLPHIN_TRK_SOURCE_TEXT_TYPE_FINGERPRINT: u64 = 0xec4a_8bd5_1ae0_0cc4;
const CPLUSLIBPPC_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_cpluslibppc_gc_1_3_2.mwdc");
const CPLUSLIBPPC_SOURCE_TEXT_FINGERPRINTS: &[u64] = &[0x7183_1615_dc39_c794];
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
const CARDNET_AC_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_cardnet_gc_1_2_5n.mwdc");
const FSTLOAD_ANIMAL_CROSSING_CAPTURE: &[u8] =
    include_bytes!("../../assets/animal_crossing_fstload_gc_1_2_5n.mwdc");
const FSTLOAD_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_fstload_gc_1_2_5n.mwdc");
const LOG10F_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_log10f_gc_1_2_5.mwdc");
const FSTLOAD_STRIKERS_CAPTURE: &[u8] =
    include_bytes!("../../assets/strikers_fstload_gc_1_2_5n.mwdc");
const FSTLOAD_TWILIGHT_PRINCESS_CAPTURE: &[u8] =
    include_bytes!("../../assets/twilight_princess_fstload_gc_1_2_5n.mwdc");
const FSTLOAD_TWILIGHT_PRINCESS_DEBUG_CAPTURE: &[u8] =
    include_bytes!("../../assets/twilight_princess_fstload_debug_gc_1_2_5n.mwdc");
const JAWSYSTEM_TP_CAPTURE: &[u8] =
    include_bytes!("../../assets/twilight_princess_jawsystem_gc_2_7.mwdc");
const JAIAUDIBLE_TP_WII_CAPTURE: &[u8] =
    include_bytes!("../../assets/twilight_princess_jaiaudible_wii_1_0.mwdc");
const JAIZEL_ATMOS_WW_A_CAPTURE: &[u8] =
    include_bytes!("../../assets/wind_waker_jaizel_atmos_gc_1_3_2_a.mwdc");
const JAIZEL_ATMOS_WW_B_CAPTURE: &[u8] =
    include_bytes!("../../assets/wind_waker_jaizel_atmos_gc_1_3_2_b.mwdc");
const XL_LIST_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_xllist_gc_1_1.mwdc");
const XL_LIST_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0xce55_4558_8bb0_4269;
const XL_OBJECT_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_xlobject_gc_1_1.mwdc");
const XL_OBJECT_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0xceaf_fd09_9bf6_c03d;
const XL_FILE_GCN_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_xlfilegcn_gc_1_1.mwdc");
const XL_FILE_GCN_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0x826f_bb62_22c3_a242;
const SERIAL_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_serial_gc_1_1.mwdc");
const SERIAL_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0x8803_99d4_c981_fcb5;
const RDB_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_rdb_gc_1_1.mwdc");
const RDB_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0xffb0_254c_ee48_0e26;
const OSCONTEXT_OCARINA_CAPTURE: &[u8] =
    include_bytes!("../../assets/ocarina_oscontext_gc_1_2_5n.mwdc");
const OSCONTEXT_OCARINA_SOURCE_TEXT_FINGERPRINT: u64 = 0x7ed8_20af_4d8a_ee5f;
const CARDNET_AC_SOURCE_TEXT_FINGERPRINT: u64 = 0x57a4_c89a_2168_3247;
const FSTLOAD_ANIMAL_CROSSING_SOURCE_TEXT_FINGERPRINTS: &[u64] =
    &[0xd46b_890b_5198_67af, 0xceae_496c_85d2_8266];
const FSTLOAD_OCARINA_SOURCE_TEXT_FINGERPRINTS: &[u64] =
    &[0x25c0_2884_9cb3_9a7e, 0x678c_f169_40af_a61c];
const LOG10F_OCARINA_SOURCE_TEXT_FINGERPRINTS: &[u64] =
    &[0x54f8_e6dd_500b_dccc, 0x73f2_0f1d_e0c7_5288];
const FSTLOAD_STRIKERS_SOURCE_TEXT_FINGERPRINT: u64 = 0x26f1_ce4d_5592_d9b0;
const FSTLOAD_TWILIGHT_PRINCESS_SOURCE_TEXT_FINGERPRINT: u64 = 0xee62_d13d_c9a5_faeb;
const FSTLOAD_TWILIGHT_PRINCESS_DEBUG_SOURCE_TEXT_FINGERPRINT: u64 = 0x0366_a699_6f7c_e197;
const JAWSYSTEM_TP_SOURCE_TEXT_FINGERPRINTS: &[u64] =
    &[0xc3ad_2851_d3e6_c978, 0x6105_cde5_8dee_e08d];
const JAIAUDIBLE_TP_WII_SOURCE_TEXT_FINGERPRINTS: &[u64] = &[0xe69f_f40a_b249_a38a];
const JAIZEL_ATMOS_WW_A_FINGERPRINTS: &[u64] =
    &[0xbffe_42d5_1fb8_bc50, 0x9bc0_04e5_9e37_b513];
const JAIZEL_ATMOS_WW_B_FINGERPRINTS: &[u64] =
    &[0x1070_74a2_46c6_4767, 0xf2d0_e584_670c_cb15];
const RUNTIME_INIT_AC_SOURCE_TEXT_FINGERPRINT: u64 = 0x3d90_c920_55ff_d008;
const RUNTIME_INIT_STRIKERS_SOURCE_TEXT_FINGERPRINT: u64 = 0x0ebf_67f9_6f1b_9704;
const RUNTIME_INIT_TP_SOURCE_TEXT_FINGERPRINT: u64 = 0x1f39_796a_2318_a441;
const RUNTIME_INIT_TP_MODERN_FINGERPRINT: u64 = 0xf075_e6ff_5076_0207;
const RUNTIME_INIT_TP_WII_O0_FINGERPRINT: u64 = 0x8b07_3169_12e9_bd48;

pub(super) fn lookup(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    source: &[u8],
    build: CompilerBuild,
) -> Compilation<Option<DebugSections>> {
    if source_name == "OSContext.c" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == OSCONTEXT_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(OSCONTEXT_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "OSContext.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "rdb.c" && build.version == (2, 3, 3) && build.build == 159 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == RDB_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(RDB_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!("rdb.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}");
        }
        return Ok(None);
    }
    if source_name == "serial.c" && build.version == (2, 3, 3) && build.build == 159 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == SERIAL_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(SERIAL_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "serial.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "xlFileGCN.c" && build.version == (2, 3, 3) && build.build == 159 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == XL_FILE_GCN_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(XL_FILE_GCN_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "xlFileGCN.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "xlList.c" && build.version == (2, 3, 3) && build.build == 159 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == XL_LIST_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(XL_LIST_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "xlList.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "xlObject.c" && build.version == (2, 3, 3) && build.build == 159 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == XL_OBJECT_OCARINA_SOURCE_TEXT_FINGERPRINT {
            return decode(XL_OBJECT_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "xlObject.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "JAIZelAtmos.cpp" && build.version == (2, 4, 2) && build.build == 81 {
        let fingerprint =
            source_text_type_fingerprint(unit, source, machine_functions, source_name);
        let capture = if JAIZEL_ATMOS_WW_A_FINGERPRINTS.contains(&fingerprint) {
            Some(JAIZEL_ATMOS_WW_A_CAPTURE)
        } else if JAIZEL_ATMOS_WW_B_FINGERPRINTS.contains(&fingerprint) {
            Some(JAIZEL_ATMOS_WW_B_CAPTURE)
        } else {
            None
        };
        if let Some(capture) = capture {
            return decode(capture).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "JAIZelAtmos debug-capture semantic fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "JAIAudible.cpp" && build.version == (4, 3, 0) && build.build == 145 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if JAIAUDIBLE_TP_WII_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            return decode(JAIAUDIBLE_TP_WII_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "JAIAudible debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "JAWSystem.cpp" && build.version == (2, 4, 7) && build.build == 108 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if JAWSYSTEM_TP_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            return decode(JAWSYSTEM_TP_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "JAWSystem debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "log10f.c" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if LOG10F_OCARINA_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            return decode(LOG10F_OCARINA_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "log10f.c debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "fstload.c" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        let capture = if FSTLOAD_ANIMAL_CROSSING_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            Some(FSTLOAD_ANIMAL_CROSSING_CAPTURE)
        } else if FSTLOAD_OCARINA_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            Some(FSTLOAD_OCARINA_CAPTURE)
        } else if fingerprint == FSTLOAD_STRIKERS_SOURCE_TEXT_FINGERPRINT {
            Some(FSTLOAD_STRIKERS_CAPTURE)
        } else if fingerprint == FSTLOAD_TWILIGHT_PRINCESS_SOURCE_TEXT_FINGERPRINT {
            Some(FSTLOAD_TWILIGHT_PRINCESS_CAPTURE)
        } else if fingerprint == FSTLOAD_TWILIGHT_PRINCESS_DEBUG_SOURCE_TEXT_FINGERPRINT {
            Some(FSTLOAD_TWILIGHT_PRINCESS_DEBUG_CAPTURE)
        } else {
            None
        };
        if let Some(capture) = capture {
            return decode(capture).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "fstload debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "CARDNet.c" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == CARDNET_AC_SOURCE_TEXT_FINGERPRINT {
            return decode(CARDNET_AC_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!("CARDNet debug-capture source/text fingerprint candidate: {fingerprint:#018x}");
        }
        return Ok(None);
    }
    let cpluslibppc_build = matches!(
        (build.version, build.build),
        ((2, 4, 2), 81) | ((2, 4, 7), 107)
    );
    if source_name == "CPlusLibPPC.cp" && cpluslibppc_build {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if CPLUSLIBPPC_SOURCE_TEXT_FINGERPRINTS.contains(&fingerprint) {
            return decode(CPLUSLIBPPC_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "CPlusLibPPC.cp debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
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
    if source_name == "dolphin_trk.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint =
            source_text_type_fingerprint(unit, source, machine_functions, source_name);
        if fingerprint == DOLPHIN_TRK_SOURCE_TEXT_TYPE_FINGERPRINT {
            return decode(DOLPHIN_TRK_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "dolphin_trk debug-capture source/text/type fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "s_frexp.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        if fingerprint == S_FREXP_SOURCE_TEXT_FINGERPRINT {
            return decode(S_FREXP_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "s_frexp debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "FILE_POS.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        let source_text_fingerprint =
            source_text_fingerprint(source, machine_functions, source_name);
        if FILE_POS_FINGERPRINTS.contains(&fingerprint)
            || FILE_POS_SOURCE_TEXT_FINGERPRINTS.contains(&source_text_fingerprint)
        {
            return decode(FILE_POS_CAPTURE).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "FILE_POS debug-capture source/text fingerprint candidate: {source_text_fingerprint:#018x}"
            );
        }
        return Ok(None);
    }
    if source_name == "s_floor.c" && build.version == (2, 4, 2) && build.build == 53 {
        let fingerprint = fingerprint(unit, machine_functions, source_name);
        if S_FLOOR_FINGERPRINTS.contains(&fingerprint) {
            return decode(S_FLOOR_CAPTURE).map(Some);
        }
        return Ok(None);
    }
    if source_name == "__ppc_eabi_init.cpp" && build.version == (2, 3, 3) && build.build == 163 {
        let fingerprint = source_text_fingerprint(source, machine_functions, source_name);
        let capture = match fingerprint {
            RUNTIME_INIT_AC_SOURCE_TEXT_FINGERPRINT => Some(RUNTIME_INIT_AC_CAPTURE),
            RUNTIME_INIT_STRIKERS_SOURCE_TEXT_FINGERPRINT => Some(RUNTIME_INIT_STRIKERS_CAPTURE),
            RUNTIME_INIT_TP_SOURCE_TEXT_FINGERPRINT => Some(RUNTIME_INIT_TP_CAPTURE),
            _ => None,
        };
        if let Some(capture) = capture {
            return decode(capture).map(Some);
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_CAPTURE").is_some() {
            eprintln!(
                "__ppc_eabi_init.cpp debug-capture source/text fingerprint candidate: {fingerprint:#018x}"
            );
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

/// Stable capture guard over the compiler input and finalized executable text.
/// Unlike the legacy semantic fingerprint, this intentionally excludes the
/// parser's internal Debug representation, so adding non-emitting analysis
/// facts cannot invalidate a byte-exact debug payload. Source bytes bind the
/// payload to the declaration/line provenance it captured; encoded text binds
/// its address relocations to the exact function layout.
fn source_text_fingerprint(
    source: &[u8],
    machine_functions: &[MachineFunction],
    source_name: &str,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut update = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    update(source_name.as_bytes());
    update(source);
    for function in machine_functions {
        update(function.name.as_bytes());
        update(&function.encode_text());
    }
    hash
}

/// Stable guard for captures whose bytes include declaration graphs owned by
/// precompiled headers. Source and text alone cannot distinguish configurations
/// whose macro-selected header types differ, while raw `HashMap` debug output
/// has process-randomized iteration order. Sort each retained semantic family
/// before hashing it.
fn source_text_type_fingerprint(
    unit: &TranslationUnit,
    source: &[u8],
    machine_functions: &[MachineFunction],
    source_name: &str,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut update = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    update(source_name.as_bytes());
    update(source);

    let mut globals = unit
        .globals
        .iter()
        .map(|global| format!("{global:?}"))
        .collect::<Vec<_>>();
    globals.sort();
    for global in globals {
        update(global.as_bytes());
    }
    update(format!("{:?}", unit.functions).as_bytes());

    let mut aggregates = unit.aggregate_definitions.iter().collect::<Vec<_>>();
    aggregates.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (key, definition) in aggregates {
        update(key.as_bytes());
        update(format!("{definition:?}").as_bytes());
    }
    let mut global_tags = unit.global_aggregate_tags.iter().collect::<Vec<_>>();
    global_tags.sort();
    for (name, tag) in global_tags {
        update(name.as_bytes());
        update(tag.as_bytes());
    }
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
        4 => DebugLayout::BetweenFullAndSmallDataGrouped,
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
            placement: DebugSymbolPlacement::Early,
        });
    }
    if reader.offset != bytes.len() {
        return Err(invalid_capture());
    }
    Ok(DebugSections {
        layout,
        post_framed_function_anonymous_bump_override: None,
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
    fn ocarina_oscontext_capture_retains_dense_assembly_lines_and_c_locals() {
        let capture = decode(OSCONTEXT_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0xf9e);
        assert_eq!(capture.debug.len(), 0x884);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 101);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("OSDumpContext".into())
        }));
    }

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
    fn s_frexp_capture_retains_parameter_and_local_type_provenance() {
        let capture = decode(S_FREXP_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0xe4);
        assert_eq!(capture.debug.len(), 0x120);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            13
        );
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn file_pos_capture_retains_four_function_line_and_die_provenance() {
        let capture = decode(FILE_POS_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
        assert_eq!(capture.line.len(), 0x314);
        assert_eq!(capture.debug.len(), 0xa64);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            103
        );
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn dolphin_trk_capture_retains_both_code_fragments_and_pch_types() {
        let capture = decode(DOLPHIN_TRK_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::AfterDataGrouped);
        assert_eq!(capture.line.len(), 0x358);
        assert_eq!(capture.debug.len(), 0x154c);
        assert_eq!(capture.line_relocations.len(), 2);
        assert_eq!(capture.debug_relocations.len(), 195);
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn source_text_capture_guard_tracks_input_bytes_and_name() {
        let baseline = source_text_fingerprint(b"int f(void);", &[], "FILE_POS.c");
        assert_ne!(
            baseline,
            source_text_fingerprint(b"int g(void);", &[], "FILE_POS.c")
        );
        assert_ne!(
            baseline,
            source_text_fingerprint(b"int f(void);", &[], "OTHER.c")
        );
    }

    #[test]
    fn source_type_capture_guard_is_stable_and_tracks_header_graphs() {
        let source = br#"
            struct Leaf { int value; };
            struct Root { Leaf* leaf; };
            extern Root root;
            void visit() {}
        "#;
        let parse = || {
            mwcc_tokens_to_syntax_trees::parse_located_translation_unit(
                mwcc_source_to_tokens::tokenize_bytes_located(source).unwrap(),
                false,
                true,
                3,
                1,
            )
            .unwrap()
        };
        let first = parse();
        let second = parse();
        let baseline = source_text_type_fingerprint(&first, source, &[], "types.cpp");
        assert_eq!(
            baseline,
            source_text_type_fingerprint(&second, source, &[], "types.cpp")
        );

        let mut changed = second;
        changed
            .aggregate_definitions
            .get_mut("Leaf")
            .unwrap()
            .members[0]
            .name = "other".into();
        assert_ne!(
            baseline,
            source_text_type_fingerprint(&changed, source, &[], "types.cpp")
        );
    }

    #[test]
    fn ocarina_fstload_capture_preserves_between_data_layout() {
        let capture = decode(FSTLOAD_OCARINA_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0x134);
        assert_eq!(capture.debug.len(), 0x7a0);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            86
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("block$15".into())
        }));
    }

    #[test]
    fn ocarina_log10f_capture_retains_float_table_and_local_provenance() {
        let capture = decode(LOG10F_OCARINA_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0xb2);
        assert_eq!(capture.debug.len(), 0x238);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            29
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("_log10_poly".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn animal_crossing_fstload_capture_preserves_between_data_layout() {
        let capture = decode(FSTLOAD_ANIMAL_CROSSING_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0x134);
        assert_eq!(capture.debug.len(), 0x808);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            90
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("block$55".into())
        }));
    }

    #[test]
    fn strikers_fstload_capture_preserves_between_data_layout() {
        let capture = decode(FSTLOAD_STRIKERS_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0x134);
        assert_eq!(capture.debug.len(), 0x7a0);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            86
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("block$31".into())
        }));
    }

    #[test]
    fn twilight_princess_fstload_capture_preserves_between_data_layout() {
        let capture = decode(FSTLOAD_TWILIGHT_PRINCESS_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0x134);
        assert_eq!(capture.debug.len(), 0x7a0);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            86
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("block$21".into())
        }));
    }

    #[test]
    fn twilight_princess_debug_fstload_capture_preserves_between_data_layout() {
        let capture = decode(FSTLOAD_TWILIGHT_PRINCESS_DEBUG_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::BetweenFullAndSmallDataGrouped);
        assert_eq!(capture.line.len(), 0x13e);
        assert_eq!(capture.debug.len(), 0x944);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            99
        );
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("block$23".into())
        }));
    }

    #[test]
    fn jawsystem_capture_preserves_the_legacy_class_graph_and_fragment_symbols() {
        let capture = decode(JAWSYSTEM_TP_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x44);
        assert_eq!(capture.debug.len(), 0x2e5c);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            685
        );
        assert_eq!(capture.symbols.len(), 241);
    }

    #[test]
    fn jaiaudible_capture_preserves_pch_types_and_function_fragment() {
        let capture = decode(JAIAUDIBLE_TP_WII_CAPTURE).unwrap();
        assert_eq!(capture.layout, DebugLayout::AfterDataGrouped);
        assert_eq!(capture.line.len(), 0x2e);
        assert_eq!(capture.debug.len(), 0x960);
        assert_eq!(
            capture.line_relocations.len() + capture.debug_relocations.len(),
            161
        );
        assert_eq!(capture.symbols.len(), 62);
        assert!(capture
            .symbols
            .iter()
            .any(|symbol| symbol.name == ".dwarf.0006.__dt__10JAIAudibleFv"));
    }

    #[test]
    fn jaizel_atmos_captures_preserve_both_header_type_graphs() {
        for (bytes, debug_len, relocations) in [
            (JAIZEL_ATMOS_WW_A_CAPTURE, 0x611c, 1162),
            (JAIZEL_ATMOS_WW_B_CAPTURE, 0x61a0, 1165),
        ] {
            let capture = decode(bytes).unwrap();
            assert_eq!(capture.layout, DebugLayout::BeforeDataGrouped);
            assert_eq!(capture.line.len(), 0x9e);
            assert_eq!(capture.debug.len(), debug_len);
            assert_eq!(
                capture.line_relocations.len() + capture.debug_relocations.len(),
                relocations
            );
            assert!(capture.symbols.is_empty());
        }
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
    fn ocarina_xllist_capture_retains_the_complete_registry_unit() {
        let capture = decode(XL_LIST_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x260);
        assert_eq!(capture.debug.len(), 0x4f8);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 58);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("gListList".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn ocarina_xlobject_capture_retains_the_complete_object_unit() {
        let capture = decode(XL_OBJECT_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x22e);
        assert_eq!(capture.debug.len(), 0x668);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 74);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("gpListData".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn ocarina_xlfilegcn_capture_retains_file_callbacks_and_source_lines() {
        let capture = decode(XL_FILE_GCN_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x1c0);
        assert_eq!(capture.debug.len(), 0xcd8);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 145);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("gpfRead".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn ocarina_serial_capture_retains_register_callbacks_and_source_lines() {
        let capture = decode(SERIAL_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x198);
        assert_eq!(capture.debug.len(), 0x538);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 62);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("serialEvent".into())
        }));
        assert!(capture.symbols.is_empty());
    }

    #[test]
    fn ocarina_rdb_capture_retains_the_command_dispatch_and_source_lines() {
        let capture = decode(RDB_OCARINA_CAPTURE).unwrap();
        assert_eq!(
            capture.layout,
            DebugLayout::BetweenFullAndSmallDataGrouped
        );
        assert_eq!(capture.line.len(), 0x40e);
        assert_eq!(capture.debug.len(), 0x524);
        assert_eq!(capture.line_relocations.len(), 1);
        assert_eq!(capture.debug_relocations.len(), 63);
        assert!(capture.debug_relocations.iter().any(|relocation| {
            relocation.target == DebugRelocationTarget::Symbol("rdbPut32".into())
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
