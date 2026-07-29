use crate::{compile, SourceLanguage};

#[test]
fn gc11_patch_keeps_the_stack_restore_before_a_shared_lr_reload() {
    let source = br#"
        typedef unsigned char u8;
        typedef unsigned int u32;
        typedef struct Buffer {
            u32 length;
            u32 position;
            u8 data[2176];
        } Buffer;

        extern void clear(void* destination, int value, u32 size);

        void reset(Buffer* message, u8 keep_data) {
            message->length = 0;
            message->position = 0;
            if (!keep_data) {
                clear(message->data, 0, 2176);
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "linkage-epilogue.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the GC/1.1p1 shared-epilogue probe should compile");

    let expected = [
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x80, 0x01, 0x00, 0x04, // lwz r0,4(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "the saved-LR load must remain dependent on the restored stack pointer",
    );
}
