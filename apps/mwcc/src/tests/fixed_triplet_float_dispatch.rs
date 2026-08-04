use crate::{compile, SourceLanguage};

#[test]
fn lowers_a_fixed_triplet_float_dispatch_without_a_frame() {
    let source = br#"
        struct Matrix { float values[3]; };
        static unsigned char choices[] = {
            0, 1, 2, 1, 0, 2, 2, 1, 0, 0, 1, 2
        };

        static float combine(
            const struct Matrix* left,
            const struct Matrix* right,
            unsigned char selector
        ) {
            float result = 0.0f;
            unsigned char* values = choices + selector * 3;
            int i;
            for (i = 0; i < 3; i++) {
                switch (values[i]) {
                case 0: break;
                case 1: result += left->values[i]; break;
                case 2: result += left->values[i] * right->values[i]; break;
                }
            }
            return result;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "fixed-triplet-float-dispatch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the fixed float dispatch should compile");

    let expected = [
        0x54, 0xa0, 0x06, 0x3e, 0x3c, 0xa0, 0x00, 0x00, 0x1c, 0xe0, 0x00, 0x03, 0x38, 0xc5,
        0x00, 0x00, 0x38, 0x00, 0x00, 0x03, 0xc0, 0x20, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x00,
        0x7c, 0xc6, 0x3a, 0x14, 0x7c, 0x09, 0x03, 0xa6, 0x88, 0x06, 0x00, 0x00, 0x2c, 0x00,
        0x00, 0x01, 0x41, 0x82, 0x00, 0x18, 0x40, 0x80, 0x00, 0x08, 0x48, 0x00, 0x00, 0x2c,
        0x2c, 0x00, 0x00, 0x03, 0x40, 0x80, 0x00, 0x24, 0x48, 0x00, 0x00, 0x10, 0x7c, 0x03,
        0x2c, 0x2e, 0xec, 0x21, 0x00, 0x2a, 0x48, 0x00, 0x00, 0x14, 0x7c, 0x43, 0x2c, 0x2e,
        0x7c, 0x04, 0x2c, 0x2e, 0xec, 0x02, 0x00, 0x32, 0xec, 0x21, 0x00, 0x2a, 0x38, 0xa5,
        0x00, 0x04, 0x38, 0xc6, 0x00, 0x01, 0x42, 0x00, 0xff, 0xbc, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "fixed float dispatch body was not found in object: {:02x?}",
        object
    );
}
