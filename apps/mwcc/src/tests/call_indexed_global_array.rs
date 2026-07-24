use crate::{compile, SourceLanguage};

#[test]
fn scales_a_narrow_call_result_into_a_global_pointer_array() {
    let source = br#"
        extern unsigned char language(void);
        extern const char* messages[6];
        const char* compiled(void) {
            return messages[language()];
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
        "call-indexed-global-array.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a narrow call result should index a global pointer array");

    // bl language; lis r4,messages@ha; rlwinm r3,r3,2,22,29;
    // addi r0,r4,messages@l; add r3,r0,r3; lwz r3,0(r3)
    let expected = [
        0x48, 0x00, 0x00, 0x01, 0x3c, 0x80, 0x00, 0x00, 0x54, 0x63, 0x15, 0xba,
        0x38, 0x04, 0x00, 0x00, 0x7c, 0x60, 0x1a, 0x14, 0x80, 0x63, 0x00, 0x00,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
