use crate::{compile, SourceLanguage};

#[test]
fn schedules_constants_around_a_static_array_address() {
    let source = br#"
        char* helper(int, char*, unsigned int);

        char* forward(void) {
            static char buffer[64];
            return helper(0, buffer, 63);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "static-array-forward.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a large static-array address should compose with constant arguments");

    // stwu; mflr; lis r4,buffer; li r3,0; save LR;
    // addi r4,r4,buffer; li r5,63; bl helper
    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x80, 0x00, 0x00, 0x38, 0x60, 0x00,
        0x00, 0x90, 0x01, 0x00, 0x14, 0x38, 0x84, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x3f, 0x48, 0x00,
        0x00, 0x01,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
