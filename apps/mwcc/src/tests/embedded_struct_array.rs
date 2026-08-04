use crate::{compile, SourceLanguage};

#[test]
fn folds_constant_inline_struct_rows_and_promotes_a_byte_switch() {
    let source = br#"
        struct Row { float values[3]; };
        struct Owner {
            unsigned char prefix[188];
            struct Row rows[4];
            float pitch_modifier;
            float volume_modifier;
        };

        void update(struct Owner* owner, unsigned char id, float value) {
            switch (id) {
            case 1:
                owner->pitch_modifier *= value;
                break;
            case 0:
                owner->volume_modifier *= value;
                break;
            case 2:
                owner->rows[1].values[1] = value;
                break;
            case 3:
                owner->rows[2].values[1] = value;
                break;
            case 4:
                owner->rows[3].values[1] = value;
                break;
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
        "embedded-struct-array.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the embedded struct-array switch should compile");

    let expected = [
        0x54, 0x80, 0x06, 0x3e, 0x2c, 0x00, 0x00, 0x02, 0x41, 0x82, 0x00, 0x48,
        0x40, 0x80, 0x00, 0x14, 0x2c, 0x00, 0x00, 0x00, 0x41, 0x82, 0x00, 0x2c,
        0x40, 0x80, 0x00, 0x18, 0x4e, 0x80, 0x00, 0x20, 0x2c, 0x00, 0x00, 0x04,
        0x41, 0x82, 0x00, 0x3c, 0x4c, 0x80, 0x00, 0x20, 0x48, 0x00, 0x00, 0x2c,
        0xc0, 0x03, 0x00, 0xec, 0xec, 0x00, 0x00, 0x72, 0xd0, 0x03, 0x00, 0xec,
        0x4e, 0x80, 0x00, 0x20, 0xc0, 0x03, 0x00, 0xf0, 0xec, 0x00, 0x00, 0x72,
        0xd0, 0x03, 0x00, 0xf0, 0x4e, 0x80, 0x00, 0x20, 0xd0, 0x23, 0x00, 0xcc,
        0x4e, 0x80, 0x00, 0x20, 0xd0, 0x23, 0x00, 0xd8, 0x4e, 0x80, 0x00, 0x20,
        0xd0, 0x23, 0x00, 0xe4, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "embedded struct-array switch body was not found: {:02x?}",
        object
    );
}
