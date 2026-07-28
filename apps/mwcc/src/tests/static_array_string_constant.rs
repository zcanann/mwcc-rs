use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_static_array_packed_string_and_constant() {
    let source = br#"
        int format(char*, const char*, ...);

        char* render(void) {
            static char buffer[12];
            format(buffer, "%d", 0);
            return buffer;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    flags.string_literals_packed = true;
    let object = compile(
        source,
        "static-array-string-constant.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("static array, packed string, and constant arguments should compose");

    // stwu; mflr; lis r3,buffer; lis r4,string; save LR;
    // addi r3; li r5,0; addi r4; crclr; bl format
    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x60, 0x00, 0x00, 0x3c, 0x80, 0x00,
        0x00, 0x90, 0x01, 0x00, 0x14, 0x38, 0x63, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x00, 0x38, 0x84,
        0x00, 0x00, 0x4c, 0xc6, 0x31, 0x82, 0x48, 0x00, 0x00, 0x01,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
