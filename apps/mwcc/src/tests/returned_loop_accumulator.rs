use crate::{compile, SourceLanguage};

#[test]
fn coalesces_a_returned_loop_accumulator_into_r3() {
    let source = br#"
        struct Node { int padding[9]; struct Node* next; };

        int count(struct Node** head) {
            struct Node* node = *head;
            int total = 0;
            while (1) {
                if (node == 0) {
                    break;
                }
                node = node->next;
                total++;
            }
            return total;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "returned-loop-accumulator.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the returned loop accumulator should compile");

    let expected = [
        0x80, 0x83, 0x00, 0x00, // lwz r4,0(r3)
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x28, 0x04, 0x00, 0x00, // cmplwi r4,0
        0x4d, 0x82, 0x00, 0x20, // beqlr
        0x80, 0x84, 0x00, 0x24, // lwz r4,36(r4)
        0x38, 0x63, 0x00, 0x01, // addi r3,r3,1
        0x4b, 0xff, 0xff, 0xf0, // b -16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "returned accumulator body was not found in object: {:02x?}",
        object
    );
}
