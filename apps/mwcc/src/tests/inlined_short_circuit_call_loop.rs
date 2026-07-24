use crate::{compile, SourceLanguage};

#[test]
fn matches_a_multi_use_helper_expanded_inside_a_list_walk() {
    let source = br#"
        typedef struct Object Object;
        typedef struct Link {
            Object* object;
            struct Link* next;
        } Link;
        struct Root { Link* head; };
        extern struct Root root;
        extern int predicate(Object*, int, int);
        extern void consume(Object*);
        static void helper(Object* object, int first, int second) {
            if (predicate(object, first, second) && first) {
                consume(object);
            }
        }
        void other(Object* object, int first, int second) {
            helper(object, first, second);
        }
        void compiled(int first, int second) {
            Link* iterator = root.head;
            while (iterator != 0) {
                Object* object;
                object = iterator->object;
                helper(object, first, second);
                iterator = iterator->next;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "inlined-short-circuit-call-loop.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the expanded list-walk helper should compile");

    // Exact GC/1.2.5n code for `compiled`, measured with mwcceppc. Relocation
    // fields remain zero in the relocatable object, so this checks the complete
    // instruction schedule independently of the other emitted functions.
    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe0,
        0x93, 0xe1, 0x00, 0x1c, 0x93, 0xc1, 0x00, 0x18, 0x93, 0xa1, 0x00, 0x14,
        0x3b, 0xa4, 0x00, 0x00, 0x93, 0x81, 0x00, 0x10, 0x3b, 0x83, 0x00, 0x00,
        0x83, 0xe0, 0x00, 0x00, 0x48, 0x00, 0x00, 0x34, 0x83, 0xdf, 0x00, 0x00,
        0x38, 0x9c, 0x00, 0x00, 0x38, 0xbd, 0x00, 0x00, 0x38, 0x7e, 0x00, 0x00,
        0x48, 0x00, 0x00, 0x01, 0x2c, 0x03, 0x00, 0x00, 0x41, 0x82, 0x00, 0x14,
        0x2c, 0x1c, 0x00, 0x00, 0x41, 0x82, 0x00, 0x0c, 0x7f, 0xc3, 0xf3, 0x78,
        0x48, 0x00, 0x00, 0x01, 0x83, 0xff, 0x00, 0x04, 0x28, 0x1f, 0x00, 0x00,
        0x40, 0x82, 0xff, 0xcc, 0x80, 0x01, 0x00, 0x24, 0x83, 0xe1, 0x00, 0x1c,
        0x83, 0xc1, 0x00, 0x18, 0x83, 0xa1, 0x00, 0x14, 0x83, 0x81, 0x00, 0x10,
        0x38, 0x21, 0x00, 0x20, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
