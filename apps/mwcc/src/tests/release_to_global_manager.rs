use crate::{compile, SourceLanguage};

#[test]
fn retains_object_and_global_manager_across_list_publication() {
    let source = br#"
        struct Object;
        struct Manager {
            unsigned int count;
            struct Object* unused;
            struct Object* free_objects;
        };
        struct Object {
            struct Object* next;
            struct Manager* manager;
        };

        static struct Manager manager;
        extern void add_object(struct Object** list, struct Object* object);

        int release_object(struct Object* object) {
            add_object(&manager.free_objects, object);
            object->manager->count--;
            manager.count++;
            object->manager = &manager;
            return 0;
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
        "release-to-global-manager.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the global-manager release should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x80, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21,
        0xff, 0xe8, 0xbf, 0xc1, 0x00, 0x10, 0x3b, 0xc3, 0x00, 0x00, 0x3b, 0xe4, 0x00, 0x00,
        0x38, 0x9e, 0x00, 0x00, 0x38, 0x7f, 0x00, 0x08, 0x48, 0x00, 0x00, 0x01, 0x80, 0xbe,
        0x00, 0x04, 0x38, 0x60, 0x00, 0x00, 0x80, 0x85, 0x00, 0x00, 0x38, 0x04, 0xff, 0xff,
        0x90, 0x05, 0x00, 0x00, 0x80, 0x9f, 0x00, 0x00, 0x38, 0x04, 0x00, 0x01, 0x90, 0x1f,
        0x00, 0x00, 0x93, 0xfe, 0x00, 0x04, 0x80, 0x01, 0x00, 0x1c, 0xbb, 0xc1, 0x00, 0x10,
        0x38, 0x21, 0x00, 0x18, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "global-manager release body was not found in object: {:02x?}",
        object
    );
}
