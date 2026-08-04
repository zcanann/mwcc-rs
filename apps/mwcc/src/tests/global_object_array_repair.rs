use crate::{compile, SourceLanguage};

#[test]
fn strength_reduces_a_call_bearing_global_object_walk() {
    let source = br#"
        struct Match { int words[4]; };
        struct Manager { int count; void* pad; struct Object* free; };
        struct Object {
            int prefix;
            struct Manager* manager;
            int padding[6];
            struct Match* match_value;
            int tail[71];
        };
        static struct Object objects[256];
        extern void stop(struct Object* object);
        extern int cut(struct Object* object);
        extern void append(struct Object** list, struct Object* object);

        static void repair(struct Match* match_value) {
            unsigned int i;
            struct Object* object;
            for (i = 0; i < 256; i++) {
                object = &objects[i];
                if (object->match_value == match_value) {
                    stop(object);
                    if (cut(object) != -1) {
                        append(&object->manager->free, object);
                    }
                }
            }
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
        "global-object-array-repair.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the global object repair should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x80, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21,
        0xff, 0xd8, 0xbf, 0x61, 0x00, 0x14, 0x3b, 0x63, 0x00, 0x00, 0x3b, 0xc4, 0x00, 0x00,
        0x3b, 0xa0, 0x00, 0x00, 0x3b, 0xe0, 0x00, 0x00, 0x7c, 0x7e, 0xfa, 0x14, 0x80, 0x03,
        0x00, 0x20, 0x3b, 0x83, 0x00, 0x00, 0x7c, 0x00, 0xd8, 0x40, 0x40, 0x82, 0x00, 0x2c,
        0x7f, 0x83, 0xe3, 0x78, 0x48, 0x00, 0x00, 0x01, 0x7f, 0x83, 0xe3, 0x78, 0x48, 0x00,
        0x00, 0x01, 0x2c, 0x03, 0xff, 0xff, 0x41, 0x82, 0x00, 0x14, 0x80, 0x7c, 0x00, 0x04,
        0x38, 0x9c, 0x00, 0x00, 0x38, 0x63, 0x00, 0x08, 0x48, 0x00, 0x00, 0x01, 0x3b, 0xbd,
        0x00, 0x01, 0x3b, 0xff, 0x01, 0x40, 0x28, 0x1d, 0x01, 0x00, 0x41, 0x80, 0xff, 0xb8,
        0xbb, 0x61, 0x00, 0x14, 0x80, 0x01, 0x00, 0x2c, 0x38, 0x21, 0x00, 0x28, 0x7c, 0x08,
        0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "global object repair body was not found in object: {:02x?}",
        object
    );
}
