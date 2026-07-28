use crate::{compile, SourceLanguage};

#[test]
fn indexes_a_static_pointer_table_with_a_global_scalar() {
    let source = br#"
        enum CurrentPlayer {
            SpongeBob,
            Patrick,
            Sandy
        };

        extern enum CurrentPlayer gCurrentPlayer;

        char* select_text(void) {
            static char* table[3] = {
                "spongebob",
                "patrick",
                "sandy"
            };
            return table[gCurrentPlayer];
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
        "global-indexed-static-table.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a global scalar should index a static pointer table");

    // lwz r0,gCurrentPlayer@sda21; lis r3,table; addi r3,r3,table;
    // slwi r0,r0,2; lwzx r3,r3,r0; blr
    let body = [
        0x80, 0x00, 0x00, 0x00, 0x3c, 0x60, 0x00, 0x00, 0x38, 0x63, 0x00, 0x00, 0x54, 0x00, 0x10,
        0x3a, 0x7c, 0x63, 0x00, 0x2e, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(body.len()).any(|bytes| bytes == body));
}
