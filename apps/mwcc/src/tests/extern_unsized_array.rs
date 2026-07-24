use crate::{compile, SourceLanguage};

#[test]
fn matches_a_call_through_an_unsized_extern_table() {
    let source = br#"
        typedef struct HSD_GObj HSD_GObj;
        typedef struct Fighter Fighter;
        struct HSD_GObj {
            unsigned char pad[44];
            Fighter* user_data;
        };
        struct Fighter {
            unsigned char pad0[4];
            int kind;
            unsigned char pad8[6516];
            HSD_GObj* interaction;
            HSD_GObj* victim;
        };
        extern void (*callbacks[])(HSD_GObj*);
        void compiled(HSD_GObj* object) {
            Fighter* fighter = object->user_data;
            if (fighter->interaction == 0 || fighter->victim == 0) {
                if (callbacks[fighter->kind] != 0) {
                    callbacks[fighter->kind](object);
                }
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "extern-unsized-array.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an unsized extern function-pointer array should compile");

    // Exact GC/1.2.5n code measured from ftCommon_8007F8E8. Relocated fields
    // remain zero in the object, so this also proves that the unknown extent
    // selects ADDR16_HA/LO rather than the small-data address form.
    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xf8,
        0x80, 0x83, 0x00, 0x2c, 0x80, 0x04, 0x19, 0x7c, 0x28, 0x00, 0x00, 0x00,
        0x41, 0x82, 0x00, 0x10, 0x80, 0x04, 0x19, 0x80, 0x28, 0x00, 0x00, 0x00,
        0x40, 0x82, 0x00, 0x2c, 0x80, 0xa4, 0x00, 0x04, 0x3c, 0x80, 0x00, 0x00,
        0x38, 0x04, 0x00, 0x00, 0x54, 0xa4, 0x10, 0x3a, 0x7c, 0x80, 0x22, 0x14,
        0x81, 0x84, 0x00, 0x00, 0x28, 0x0c, 0x00, 0x00, 0x41, 0x82, 0x00, 0x0c,
        0x7d, 0x88, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x21, 0x80, 0x01, 0x00, 0x0c,
        0x38, 0x21, 0x00, 0x08, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
