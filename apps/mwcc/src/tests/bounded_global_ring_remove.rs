use crate::{compile, SourceLanguage};

#[test]
fn uses_one_scaled_ring_offset_for_lookup_and_removal() {
    let source = br#"
        struct Item { int value; };
        static unsigned int count;
        static unsigned int top;
        static struct Item* items[32];

        int remove_item(struct Item* item) {
            unsigned int i;
            unsigned int slot;
            for (i = 0; i < count; i++) {
                slot = (top + i) & 31;
                if (items[slot] == item) {
                    items[slot] = 0;
                    return 1;
                }
            }
            return 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "bounded-global-ring-remove.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the bounded ring removal should compile");

    let expected = [
        0x80, 0x00, 0x00, 0x00, 0x3c, 0x80, 0x00, 0x00, 0x38, 0xa4, 0x00, 0x00, 0x80, 0xe0,
        0x00, 0x00, 0x39, 0x00, 0x00, 0x00, 0x7c, 0x09, 0x03, 0xa6, 0x28, 0x00, 0x00, 0x00,
        0x40, 0x81, 0x00, 0x40, 0x7c, 0x07, 0x42, 0x14, 0x54, 0x06, 0x16, 0x7a, 0x7c, 0x85,
        0x32, 0x14, 0x80, 0x04, 0x00, 0x00, 0x7c, 0x00, 0x18, 0x40, 0x40, 0x82, 0x00, 0x20,
        0x3c, 0x60, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x00, 0x38, 0x03, 0x00, 0x00, 0x38, 0x60,
        0x00, 0x01, 0x7c, 0x80, 0x32, 0x14, 0x90, 0xa4, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        0x39, 0x08, 0x00, 0x01, 0x42, 0x00, 0xff, 0xc8, 0x38, 0x60, 0x00, 0x00, 0x4e, 0x80,
        0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "bounded ring removal body was not found in object: {:02x?}",
        object
    );
}
