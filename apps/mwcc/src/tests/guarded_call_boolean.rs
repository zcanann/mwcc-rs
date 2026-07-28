use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_guarded_call_boolean_diamond() {
    let source = br#"
        typedef int bool;
        extern bool release(void**);

        bool close_file(void** output) {
            if (!release((void**)output)) {
                return 0;
            }
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "guarded-call-boolean.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the guarded call boolean should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xf8, 0x48, 0x00, 0x00,
        0x01, 0x2c, 0x03, 0x00, 0x00, 0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00,
        0x00, 0x08, 0x38, 0x60, 0x00, 0x01, 0x80, 0x01, 0x00, 0x0c, 0x38, 0x21, 0x00, 0x08, 0x7c,
        0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
