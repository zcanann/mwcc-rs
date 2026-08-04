use crate::{compile, SourceLanguage};

#[test]
fn initializes_a_non_power_of_two_global_struct_array() {
    let source = br#"
        struct Element;
        struct Owner {
            unsigned int count;
            unsigned int reserved;
            struct Element* first;
            unsigned char tail[104];
        };
        struct Element {
            unsigned int prefix;
            struct Owner* owner;
            unsigned char tail[312];
        };
        static struct Owner global_owner;
        static struct Element elements[256];
        extern void initialize_owner(struct Owner* owner);
        extern void initialize_element(struct Element* element);
        extern void append_element(struct Element** list, struct Element* element);

        void initialize_all(void) {
            struct Owner* owner;
            int i;
            owner = &global_owner;
            initialize_owner(owner);
            for (i = 0; i < 256; i++) {
                initialize_element(&elements[i]);
                append_element(&owner->first, &elements[i]);
                elements[i].owner = owner;
            }
            owner->count = 256;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.use_lmw_stmw = true;
    let object = compile(
        source,
        "global-struct-array-initialization.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the global struct-array initialization should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x60, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04,
        0x38, 0x03, 0x00, 0x00, 0x94, 0x21, 0xff, 0xe0, 0xbf, 0x61, 0x00, 0x0c,
        0x7c, 0x1c, 0x03, 0x78, 0x38, 0x7c, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01,
        0x3c, 0x60, 0x00, 0x00, 0x3b, 0x60, 0x00, 0x00, 0x3b, 0xc3, 0x00, 0x00,
        0x3b, 0xe0, 0x00, 0x00, 0x7f, 0xbe, 0xfa, 0x14, 0x38, 0x7d, 0x00, 0x00,
        0x48, 0x00, 0x00, 0x01, 0x38, 0x7c, 0x00, 0x08, 0x38, 0x9d, 0x00, 0x00,
        0x48, 0x00, 0x00, 0x01, 0x3b, 0x7b, 0x00, 0x01, 0x93, 0x9d, 0x00, 0x04,
        0x2c, 0x1b, 0x01, 0x00, 0x3b, 0xff, 0x01, 0x40, 0x41, 0x80, 0xff, 0xd8,
        0x38, 0x00, 0x01, 0x00, 0x90, 0x1c, 0x00, 0x00, 0xbb, 0x61, 0x00, 0x0c,
        0x80, 0x01, 0x00, 0x24, 0x38, 0x21, 0x00, 0x20, 0x7c, 0x08, 0x03, 0xa6,
        0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "global struct-array initialization body was not found: {:02x?}",
        object
    );
}
