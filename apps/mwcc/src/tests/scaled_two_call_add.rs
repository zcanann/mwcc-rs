use crate::{compile, SourceLanguage};

#[test]
fn evaluates_and_saves_the_scaled_call_result_first() {
    let source = br#"
        extern unsigned day(void);
        extern unsigned month(void);

        unsigned date(void) {
            return day() + month() * 100;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "scaled-two-call-add.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a scaled call result should survive the second call");

    let expected = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x14, 0x93, 0xe1, 0x00,
        0x0c, 0x48, 0x00, 0x00, 0x01, 0x1f, 0xe3, 0x00, 0x64, 0x48, 0x00, 0x00, 0x01, 0x80, 0x01,
        0x00, 0x14, 0x7c, 0x63, 0xfa, 0x14, 0x83, 0xe1, 0x00, 0x0c, 0x7c, 0x08, 0x03, 0xa6, 0x38,
        0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
