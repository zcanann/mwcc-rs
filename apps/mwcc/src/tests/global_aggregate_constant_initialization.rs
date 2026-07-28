use crate::{compile, SourceLanguage};

#[test]
fn shares_the_bss_base_across_a_reused_constant_initialization() {
    let source = br#"
        static int pad[4];
        typedef struct {
            int size;
            int count;
            void* head;
            void* next;
        } State;
        static State state;

        int read_pad(void) {
            return pad[0];
        }

        int setup(void) {
            state.count = 0;
            state.size = 16;
            state.next = 0;
            state.head = 0;
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "global-aggregate-constant-initialization.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the aggregate initialization should compile");

    // `read_pad` places `pad` first in .bss, so every state member displacement
    // includes state's 0x10 section offset while both address relocations still
    // target the zero-offset `...bss.0` anchor.
    let expected = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,...bss.0@ha
        0x38, 0xa3, 0x00, 0x00, // addi r5,r3,...bss.0@l
        0x38, 0x80, 0x00, 0x00, // li r4,0
        0x90, 0x85, 0x00, 0x14, // stw r4,0x14(r5)
        0x38, 0x00, 0x00, 0x10, // li r0,16
        0x38, 0x60, 0x00, 0x01, // li r3,1
        0x90, 0x05, 0x00, 0x10, // stw r0,0x10(r5)
        0x90, 0x85, 0x00, 0x1c, // stw r4,0x1c(r5)
        0x90, 0x85, 0x00, 0x18, // stw r4,0x18(r5)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
    assert!(object.windows(9).any(|bytes| bytes == b"...bss.0\0"));
}
