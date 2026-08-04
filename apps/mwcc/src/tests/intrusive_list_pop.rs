use crate::{compile, SourceLanguage};

#[test]
fn pops_and_detaches_an_intrusive_list_head() {
    let source = br#"
        struct Node {
            int prefix[2];
            struct Node** owner;
            int gap[6];
            struct Node* next;
            unsigned char tail[280];
        };

        struct Node* pop_head(struct Node** head) {
            struct Node* node = *head;
            if (node == 0)
                return 0;
            *head = node->next;
            node->owner = 0;
            return node;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "intrusive-list-pop.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the intrusive list pop should compile");

    let expected = [
        0x80, 0x03, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x7c, 0x05, 0x03, 0x78,
        0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
        0x80, 0x85, 0x00, 0x24, 0x38, 0x00, 0x00, 0x00, 0x90, 0x83, 0x00, 0x00,
        0x7c, 0xa3, 0x2b, 0x78, 0x90, 0x05, 0x00, 0x08, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "intrusive list-pop body was not found in object: {:02x?}",
        object
    );
}
