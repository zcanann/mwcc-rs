use crate::{compile, SourceLanguage};

#[test]
fn preserves_a_member_preincrement_beside_the_comparison_load() {
    let source = br#"
        struct Manager {
            int slot_count;
            int padding[3];
            int current;
        };

        void update(struct Manager* manager) {
            if (++manager->current >= manager->slot_count) {
                manager->current = 0;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "member-preincrement-condition.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the preincrement comparison should compile");

    let expected = [
        0x80, 0x83, 0x00, 0x10, // lwz r4,16(r3)
        0x38, 0x84, 0x00, 0x01, // addi r4,r4,1
        0x90, 0x83, 0x00, 0x10, // stw r4,16(r3)
        0x80, 0x03, 0x00, 0x00, // lwz r0,0(r3)
        0x7c, 0x04, 0x00, 0x00, // cmpw r4,r0
        0x4d, 0x80, 0x00, 0x20, // bltlr
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x03, 0x00, 0x10, // stw r0,16(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
