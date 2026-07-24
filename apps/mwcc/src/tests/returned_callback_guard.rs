use crate::{compile, SourceLanguage};

#[test]
fn preserves_a_call_result_through_a_returned_callback_guard() {
    let source = br#"
        typedef int BOOL;
        extern BOOL disable(void);
        extern void restore(BOOL enabled);
        static void show(void) {}
        static void (*active)(void);

        int compiled(BOOL enable) {
            BOOL enabled;
            int previous;
            enabled = disable();
            previous = active ? 1 : 0;
            active = enable ? show : 0;
            restore(enabled);
            return previous;
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
        "returned-callback-guard.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the callback guard should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xe8, // stwu r1,-24(r1)
        0x93, 0xe1, 0x00, 0x14, // stw r31,20(r1)
        0x93, 0xc1, 0x00, 0x10, // stw r30,16(r1)
        0x7c, 0x7e, 0x1b, 0x78, // mr r30,r3
        0x48, 0x00, 0x00, 0x01, // bl disable
        0x80, 0x00, 0x00, 0x00, // lwz r0,active@sda21
        0x28, 0x00, 0x00, 0x00, // cmplwi r0,0
        0x41, 0x82, 0x00, 0x0c, // beq false
        0x3b, 0xe0, 0x00, 0x01, // li r31,1
        0x48, 0x00, 0x00, 0x08, // b join
        0x3b, 0xe0, 0x00, 0x00, // li r31,0
        0x2c, 0x1e, 0x00, 0x00, // cmpwi r30,0
        0x41, 0x82, 0x00, 0x10, // beq false
        0x3c, 0x80, 0x00, 0x00, // lis r4,show@ha
        0x38, 0x04, 0x00, 0x00, // addi r0,r4,show@l
        0x48, 0x00, 0x00, 0x08, // b join
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x00, 0x00, 0x00, // stw r0,active@sda21
        0x48, 0x00, 0x00, 0x01, // bl restore
        0x7f, 0xe3, 0xfb, 0x78, // mr r3,r31
        0x80, 0x01, 0x00, 0x1c, // lwz r0,28(r1)
        0x83, 0xe1, 0x00, 0x14, // lwz r31,20(r1)
        0x83, 0xc1, 0x00, 0x10, // lwz r30,16(r1)
        0x38, 0x21, 0x00, 0x18, // addi r1,r1,24
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
