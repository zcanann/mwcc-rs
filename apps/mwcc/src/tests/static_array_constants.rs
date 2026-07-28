use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_static_array_address_with_two_constants() {
    let source = br#"
        void clear(char*, int, unsigned int);

        char* reset(void) {
            static char buffer[64];
            clear(buffer, 0, 64);
            return buffer;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "static-array-constants.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a static-array address should compose with two constant arguments");

    // stwu; mflr; lis r3,buffer; li r4,0; save LR;
    // addi r3,r3,buffer; li r5,64; bl clear
    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x60, 0x00, 0x00, 0x38, 0x80, 0x00,
        0x00, 0x90, 0x01, 0x00, 0x14, 0x38, 0x63, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x40, 0x48, 0x00,
        0x00, 0x01,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
