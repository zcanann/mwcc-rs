use crate::{compile, SourceLanguage};

#[test]
fn selects_between_float_members_into_a_member_store() {
    let source = br#"
        struct Frame {
            float maximum;
            float minimum;
            float current;
            int kind;
        };

        void init_frame(struct Frame* frame) {
            frame->current = frame->kind == 1 ? frame->maximum : frame->minimum;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_3_0A3,
        flags,
    };
    let object = compile(
        source,
        "member-float-select.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("two float members should form a load-select-store diamond");

    let expected = [
        0x80, 0x03, 0x00, 0x0c, // lwz r0,12(r3)
        0x2c, 0x00, 0x00, 0x01, // cmpwi r0,1
        0x40, 0x82, 0x00, 0x0c, // bne false
        0xc0, 0x03, 0x00, 0x00, // lfs f0,0(r3)
        0x48, 0x00, 0x00, 0x08, // b join
        0xc0, 0x03, 0x00, 0x04, // false: lfs f0,4(r3)
        0xd0, 0x03, 0x00, 0x08, // join: stfs f0,8(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
