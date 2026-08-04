use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef struct Thread Thread;
    typedef struct Mutex Mutex;
    struct Thread { char padding[752]; Mutex* mutex; };
    struct Mutex { char padding[8]; Thread* thread; };

    int check_deadlock(Thread* thread) {
        Mutex* mutex = thread->mutex;
        while (mutex && mutex->thread) {
            if (mutex->thread == thread) {
                return 1;
            }
            mutex = mutex->thread->mutex;
        }
        return 0;
    }
"#;

#[test]
fn reuses_the_cursor_for_each_nested_owner_link() {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        SOURCE,
        "nested-pointer-search.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the nested owner search should compile");

    let expected = [
        0x80, 0x83, 0x02, 0xf0, // lwz r4,752(r3)
        0x48, 0x00, 0x00, 0x18, // b test
        0x7c, 0x04, 0x18, 0x40, // cmplw r4,r3
        0x40, 0x82, 0x00, 0x0c, // bne chase
        0x38, 0x60, 0x00, 0x01, // li r3,1
        0x4e, 0x80, 0x00, 0x20, // blr
        0x80, 0x84, 0x02, 0xf0, // lwz r4,752(r4)
        0x28, 0x04, 0x00, 0x00, // cmplwi r4,0
        0x41, 0x82, 0x00, 0x10, // beq missing
        0x80, 0x84, 0x00, 0x08, // lwz r4,8(r4)
        0x28, 0x04, 0x00, 0x00, // cmplwi r4,0
        0x40, 0x82, 0xff, 0xdc, // bne body
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
