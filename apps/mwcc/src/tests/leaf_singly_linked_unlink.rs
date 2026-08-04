use crate::{compile, SourceLanguage};

#[test]
fn caches_the_head_cursor_and_count_for_a_leaf_unlink() {
    let source = br#"
        struct Node {
            int prefix[2];
            struct Node** owner;
            int middle[6];
            struct Node* next;
        };

        int unlink_node(struct Node* node) {
            struct Node* cursor = *node->owner;
            int index = 0;
            if (cursor == node) {
                *node->owner = node->next;
                node->owner = 0;
                return 0;
            }
            while (1) {
                index++;
                if (cursor == 0) {
                    return -1;
                }
                if (cursor->next == node) {
                    break;
                }
                cursor = cursor->next;
            }
            cursor->next = node->next;
            node->owner = 0;
            return index;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "leaf-singly-linked-unlink.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the leaf unlink should compile");

    let expected = [
        0x80, 0xa3, 0x00, 0x08, 0x38, 0xe0, 0x00, 0x00, 0x80, 0x05, 0x00, 0x00, 0x7c, 0x00,
        0x18, 0x40, 0x7c, 0x06, 0x03, 0x78, 0x40, 0x82, 0x00, 0x1c, 0x80, 0x83, 0x00, 0x24,
        0x38, 0x00, 0x00, 0x00, 0x90, 0x85, 0x00, 0x00, 0x90, 0x03, 0x00, 0x08, 0x38, 0x60,
        0x00, 0x00, 0x4e, 0x80, 0x00, 0x20, 0x28, 0x06, 0x00, 0x00, 0x38, 0xe7, 0x00, 0x01,
        0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0xff, 0xff, 0x4e, 0x80, 0x00, 0x20, 0x80, 0x06,
        0x00, 0x24, 0x7c, 0x00, 0x18, 0x40, 0x41, 0x82, 0x00, 0x0c, 0x7c, 0x06, 0x03, 0x78,
        0x4b, 0xff, 0xff, 0xdc, 0x80, 0x83, 0x00, 0x24, 0x38, 0x00, 0x00, 0x00, 0x90, 0x86,
        0x00, 0x24, 0x90, 0x03, 0x00, 0x08, 0x7c, 0xe3, 0x3b, 0x78, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "leaf unlink body was not found in object: {:02x?}",
        object
    );
}
