use crate::{compile, SourceLanguage};

#[test]
fn retains_both_lists_while_moving_objects_from_a_global_pool() {
    let source = br#"
        struct Object;
        struct Manager {
            unsigned int count;
            void* unused;
            struct Object* free_objects;
            int tail[26];
        };
        struct Object {
            struct Object* next;
            struct Manager* manager;
            int tail[78];
        };

        static struct Manager global_manager;
        extern struct Object* take_object(struct Object** list);
        extern void append_object(struct Object** list, struct Object* object);
        extern void initialize_object(struct Object* object);

        int allocate_objects(struct Manager* manager, unsigned int limit) {
            struct Manager** manager_alias = &manager;
            unsigned int* limit_alias = &limit;
            int count = 0;
            struct Object* object;
            while (count < limit) {
                object = take_object(&global_manager.free_objects);
                if (object == 0)
                    break;
                append_object(&manager->free_objects, object);
                object->manager = manager;
                initialize_object(object);
                count = count + 1;
            }
            manager->count += count;
            global_manager.count -= count;
            return count;
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
        "allocate-from-global-pool.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the global-pool allocation should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xc8, 0xbf, 0x21,
        0x00, 0x1c, 0x3b, 0x40, 0x00, 0x00, 0x90, 0x61, 0x00, 0x08, 0x3c, 0x60, 0x00, 0x00,
        0x3b, 0xe3, 0x00, 0x00, 0x90, 0x81, 0x00, 0x0c, 0x3b, 0xdf, 0x00, 0x08, 0x83, 0x61,
        0x00, 0x08, 0x83, 0x81, 0x00, 0x0c, 0x3b, 0xbb, 0x00, 0x08, 0x48, 0x00, 0x00, 0x30,
        0x7f, 0xc3, 0xf3, 0x78, 0x48, 0x00, 0x00, 0x01, 0x7c, 0x79, 0x1b, 0x79, 0x41, 0x82,
        0x00, 0x28, 0x38, 0x7d, 0x00, 0x00, 0x38, 0x99, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01,
        0x93, 0x79, 0x00, 0x04, 0x7f, 0x23, 0xcb, 0x78, 0x48, 0x00, 0x00, 0x01, 0x3b, 0x5a,
        0x00, 0x01, 0x7c, 0x1a, 0xe0, 0x40, 0x41, 0x80, 0xff, 0xd0, 0x80, 0x1b, 0x00, 0x00,
        0x38, 0x7a, 0x00, 0x00, 0x7c, 0x00, 0xd2, 0x14, 0x90, 0x1b, 0x00, 0x00, 0x80, 0x1f,
        0x00, 0x00, 0x7c, 0x1a, 0x00, 0x50, 0x90, 0x1f, 0x00, 0x00, 0x80, 0x01, 0x00, 0x3c,
        0xbb, 0x21, 0x00, 0x1c, 0x38, 0x21, 0x00, 0x38, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80,
        0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "global-pool allocation body was not found in object: {:02x?}",
        object
    );
}
