use crate::{compile, SourceLanguage};

#[test]
fn snapshots_a_dereferenced_struct_pointer_before_overwriting_its_source() {
    let source = br#"
        struct Node {
            int padding0[2];
            struct Node** head;
            int padding1[6];
            struct Node* next;
        };

        void add(struct Node** head, struct Node* node) {
            struct Node* old = *head;
            node->head = head;
            *head = node;
            node->next = old;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "materialized-pointer-snapshot.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the pointer snapshot store run should compile");

    let expected = [
        0x80, 0x03, 0x00, 0x00, // lwz r0,0(r3)
        0x90, 0x64, 0x00, 0x08, // stw r3,8(r4)
        0x90, 0x83, 0x00, 0x00, // stw r4,0(r3)
        0x90, 0x04, 0x00, 0x24, // stw r0,36(r4)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
