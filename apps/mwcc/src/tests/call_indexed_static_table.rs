use crate::{compile, SourceLanguage};

#[test]
fn indexes_a_static_pointer_table_with_a_call_result() {
    let source = br#"
        int get_index(void);

        char* select_text(void) {
            static char* table[4] = {
                "zero",
                "one",
                "two",
                "three"
            };
            return table[get_index()];
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
        "call-indexed-static-table.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a nested index call should retain its result through the table load");

    // bl get_index; lis r4,table; slwi r0,r3,2;
    // addi r3,r4,table; lwzx r3,r3,r0
    let schedule = [
        0x48, 0x00, 0x00, 0x01, 0x3c, 0x80, 0x00, 0x00, 0x54, 0x60, 0x10, 0x3a, 0x38, 0x64, 0x00,
        0x00, 0x7c, 0x63, 0x00, 0x2e,
    ];
    assert!(object
        .windows(schedule.len())
        .any(|bytes| bytes == schedule));
}
