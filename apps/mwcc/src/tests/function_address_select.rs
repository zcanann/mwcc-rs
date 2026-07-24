use crate::{compile, SourceLanguage};

#[test]
fn selects_a_function_address_or_null_for_a_pointer_global() {
    let source = br#"
        extern void callback(void);
        static void (*active)(void);
        void compiled(int enabled) {
            active = enabled ? callback : 0;
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
        "function-address-select.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a function-address/null select should compile");

    let expected = [
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
        0x41, 0x82, 0x00, 0x10, // beq false
        0x3c, 0x60, 0x00, 0x00, // lis r3,callback@ha
        0x38, 0x03, 0x00, 0x00, // addi r0,r3,callback@l
        0x48, 0x00, 0x00, 0x08, // b join
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x00, 0x00, 0x00, // stw r0,active@sda21
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
