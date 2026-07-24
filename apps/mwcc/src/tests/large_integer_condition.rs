use crate::{compile, SourceLanguage};

#[test]
fn compares_a_loaded_word_with_a_large_immediate_by_halves() {
    let source = br#"
        int classify(const unsigned int* value) {
            if (*value == 0x12345678) {
                return 1;
            }
            return 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "large-integer-condition.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a large integer condition should compile");

    let expected = [
        0x80, 0x63, 0x00, 0x00, // lwz r3,0(r3)
        0x3c, 0x03, 0xed, 0xcc, // addis r0,r3,-0x1234
        0x28, 0x00, 0x56, 0x78, // cmplwi r0,0x5678
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
