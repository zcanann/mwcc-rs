use crate::{compile, SourceLanguage};

#[test]
fn widens_narrow_member_loads_for_word_stores() {
    let source = br#"
        struct U16 { unsigned short source; int destination; };
        void u16_to_int(struct U16* p) { p->destination = p->source; }

        struct S16 { short source; int destination; };
        void s16_to_int(struct S16* p) { p->destination = p->source; }

        struct U8 { unsigned char source; int destination; };
        void u8_to_int(struct U8* p) { p->destination = p->source; }

        struct S8 { signed char source; int destination; };
        void s8_to_int(struct S8* p) { p->destination = p->source; }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "narrow-to-wide-store.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("narrow values should promote for word stores");

    // Exact GC/2.6 output measured from mwcceppc.
    let expected = [
        0xa0, 0x03, 0x00, 0x00, 0x90, 0x03, 0x00, 0x04, 0x4e, 0x80, 0x00, 0x20,
        0xa8, 0x03, 0x00, 0x00, 0x90, 0x03, 0x00, 0x04, 0x4e, 0x80, 0x00, 0x20,
        0x88, 0x03, 0x00, 0x00, 0x90, 0x03, 0x00, 0x04, 0x4e, 0x80, 0x00, 0x20,
        0x88, 0x03, 0x00, 0x00, 0x7c, 0x00, 0x07, 0x74, 0x90, 0x03, 0x00, 0x04,
        0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
