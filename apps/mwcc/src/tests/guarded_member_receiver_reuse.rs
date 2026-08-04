use crate::{compile, SourceLanguage};

#[test]
fn reuses_a_tested_member_as_the_guarded_call_receiver() {
    let source = br#"
        struct Manager;
        struct Context {
            struct Manager* manager;
            int slot;
            unsigned char enabled;
        };

        void remove_client(struct Manager* manager, struct Context* context);

        void exit_context(struct Context* context) {
            if (context->manager) {
                remove_client(context->manager, context);
                context->manager = 0;
            }
            context->enabled = 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "guarded-member-receiver-reuse.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the guarded member call should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xe8, // stwu r1,-24(r1)
        0x93, 0xe1, 0x00, 0x14, // stw r31,20(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0x80, 0x63, 0x00, 0x00, // lwz r3,0(r3)
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x41, 0x82, 0x00, 0x14, // beq +20
        0x7f, 0xe4, 0xfb, 0x78, // mr r4,r31
        0x48, 0x00, 0x00, 0x01, // bl remove_client
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x1f, 0x00, 0x00, // stw r0,0(r31)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x98, 0x1f, 0x00, 0x08, // stb r0,8(r31)
        0x80, 0x01, 0x00, 0x1c, // lwz r0,28(r1)
        0x83, 0xe1, 0x00, 0x14, // lwz r31,20(r1)
        0x38, 0x21, 0x00, 0x18, // addi r1,r1,24
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
