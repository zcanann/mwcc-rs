use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_leaf_singly_linked_tail_append() {
    let source = br#"
        struct Node {
            int prefix[2];
            struct Node** owner;
            int middle[6];
            struct Node* next;
        };

        void append_tail(struct Node** head, struct Node* item) {
            struct Node* cursor = *head;
            item->owner = head;
            if (cursor == 0) {
                *head = item;
                item->next = 0;
                return;
            }

            struct Node* next;
            while (1) {
                next = cursor->next;
                if (next == 0) {
                    cursor->next = item;
                    item->next = 0;
                    return;
                }
                cursor = next;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "leaf-tail-append.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the leaf tail append should compile");

    let expected = [
        0x80, 0xa3, 0x00, 0x00, // lwz r5,0(r3)
        0x28, 0x05, 0x00, 0x00, // cmplwi r5,0
        0x90, 0x64, 0x00, 0x08, // stw r3,8(r4)
        0x40, 0x82, 0x00, 0x14, // bne +20
        0x90, 0x83, 0x00, 0x00, // stw r4,0(r3)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x04, 0x00, 0x24, // stw r0,36(r4)
        0x4e, 0x80, 0x00, 0x20, // blr
        0x80, 0x05, 0x00, 0x24, // lwz r0,36(r5)
        0x28, 0x00, 0x00, 0x00, // cmplwi r0,0
        0x40, 0x82, 0x00, 0x14, // bne +20
        0x90, 0x85, 0x00, 0x24, // stw r4,36(r5)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x04, 0x00, 0x24, // stw r0,36(r4)
        0x4e, 0x80, 0x00, 0x20, // blr
        0x7c, 0x05, 0x03, 0x78, // mr r5,r0
        0x4b, 0xff, 0xff, 0xe0, // b -32
        0x4e, 0x80, 0x00, 0x20, // unreachable terminal blr
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "leaf tail append body was not found in object: {:02x?}",
        object
    );
}
