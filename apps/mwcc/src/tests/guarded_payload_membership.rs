use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_guarded_payload_membership() {
    let source = br#"
        typedef struct Type {
            int size;
            void* event;
            void* argument;
            char* name;
        } Type;
        typedef struct Payload {
            void* list;
            Type* type;
        } Payload;
        typedef struct Registry Registry;

        static Registry* registry;
        extern int test_item(Registry* list, Payload* payload);

        int test(void* object, Type* requested_type) {
            Payload* payload;
            if (object != 0) {
                payload = *(Payload**)((unsigned char*)object - 4);
                if (test_item(registry, payload) && payload->type == requested_type) {
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
    let object = compile(
        source,
        "guarded-payload-membership.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the guarded payload membership should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x28, 0x03, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff,
        0xe8, 0x93, 0xe1, 0x00, 0x14, 0x93, 0xc1, 0x00, 0x10, 0x3b, 0xc4, 0x00, 0x00, 0x41, 0x82,
        0x00, 0x30, 0x83, 0xe3, 0xff, 0xfc, 0x80, 0x60, 0x00, 0x00, 0x7f, 0xe4, 0xfb, 0x78, 0x48,
        0x00, 0x00, 0x01, 0x2c, 0x03, 0x00, 0x00, 0x41, 0x82, 0x00, 0x18, 0x80, 0x1f, 0x00, 0x04,
        0x7c, 0x00, 0xf0, 0x40, 0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x01, 0x48, 0x00, 0x00,
        0x08, 0x38, 0x60, 0x00, 0x00, 0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x83, 0xc1,
        0x00, 0x10, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x18, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
