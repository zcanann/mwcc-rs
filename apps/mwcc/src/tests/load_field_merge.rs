use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::{
    CompilerBuild, CompilerConfig, Flags, Optimization, GC_1_1, GC_1_3, GC_2_6, GC_3_0A3, WII_1_0,
};

fn object(build: CompilerBuild, optimization: Optimization) -> Vec<u8> {
    let flags = Flags {
        optimization,
        debug_info: false,
        emit_mwcats: false,
        inline_enabled: false,
        cpp_exceptions: false,
        ..Flags::default()
    };
    compile(
        br#"
        typedef unsigned char u8;
        typedef unsigned short u16;
        unsigned little(const u8* p) { return p[0] | ((unsigned)p[1] << 8); }
        unsigned big(const u8* p) { return ((unsigned)p[0] << 8) | p[1]; }
        unsigned halves(const u16* p) { return ((unsigned)p[0] << 16) | p[1]; }
        unsigned overlap(const u8* p) { return p[0] | ((unsigned)p[1] << 4); }
        unsigned signed_insert(const u8* a, const short* b) { return a[0] | ((unsigned)b[0] << 8); }
        unsigned observable(volatile u8* p) { return p[0] | ((unsigned)p[1] << 8); }
        "#,
        "load-fields.c",
        CompilerConfig { build, flags },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("load field cases should compile")
}

fn words(object: &[u8], name: &str) -> Vec<u32> {
    function_bytes(object, name)
        .chunks_exact(4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn matches_reference_byte_and_halfword_insertions() {
    // Measured mwcceppc bodies, including the source-first load placement.
    for build in [GC_1_1, GC_1_3, GC_2_6, GC_3_0A3, WII_1_0] {
        let object = object(build, Optimization::O4);
        assert_eq!(
            words(&object, "big"),
            [0x88030000, 0x88630001, 0x5003442e, 0x4e800020]
        );
        assert_eq!(
            words(&object, "halves"),
            [0xa0030000, 0xa0630002, 0x5003801e, 0x4e800020]
        );
    }
    assert_eq!(
        words(&object(GC_1_3, Optimization::O4), "little"),
        [0x88030001, 0x88630000, 0x5003442e, 0x4e800020]
    );
}

#[test]
fn preserves_overlaps_sign_extension_and_observable_load_order() {
    let object = object(GC_1_3, Optimization::O4);
    for name in ["overlap", "signed_insert", "observable"] {
        assert!(
            !words(&object, name).iter().any(|word| word >> 26 == 20),
            "{name} cannot use the nonvolatile disjoint-field owner"
        );
    }
    // The volatile control retains the existing p[0], p[1] order.
    assert_eq!(
        &words(&object, "observable")[..2],
        &[0x88830000, 0x88030001]
    );
}

#[test]
fn leaves_unoptimized_packing_to_separate_shifts_and_ors() {
    let object = object(GC_1_3, Optimization::O0);
    for name in ["little", "big", "halves"] {
        assert!(!words(&object, name).iter().any(|word| word >> 26 == 20));
    }
}
