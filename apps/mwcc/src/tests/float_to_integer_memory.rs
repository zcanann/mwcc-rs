use crate::{compile, SourceLanguage};

#[test]
fn converts_float_memory_values_to_signed_integers_byte_exactly() {
    let source = br#"
        int load_cast(float* source) {
            return (int)*source;
        }
        void store_cast(int* destination, float* source) {
            *destination = *source;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "float-to-integer-memory.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("float memory conversions should compile");

    // Exact GC/2.6 hardware-FP output measured from mwcceppc.
    let expected = [
        0x94, 0x21, 0xff, 0xf0, 0xc0, 0x03, 0x00, 0x00, 0xfc, 0x00, 0x00, 0x1e,
        0xd8, 0x01, 0x00, 0x08, 0x80, 0x61, 0x00, 0x0c, 0x38, 0x21, 0x00, 0x10,
        0x4e, 0x80, 0x00, 0x20, 0x94, 0x21, 0xff, 0xf0, 0xc0, 0x04, 0x00, 0x00,
        0xfc, 0x00, 0x00, 0x1e, 0xd8, 0x01, 0x00, 0x08, 0x80, 0x01, 0x00, 0x0c,
        0x90, 0x03, 0x00, 0x00, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
