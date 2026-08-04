use crate::{compile, SourceLanguage};

#[test]
fn retains_an_owner_beside_its_nullable_member_call_receiver() {
    let source = br#"
        struct Manager;
        struct Context {
            struct Manager* manager;
        };

        unsigned char is_updatable(struct Manager* manager, struct Context* context);

        unsigned char updatable(struct Context* context) {
            if (!context->manager) {
                return 0;
            }
            if (is_updatable(context->manager, context)) {
                return 1;
            }
            return 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "guarded-member-call-entry.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the nullable member call should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x7c, 0x64, 0x1b, 0x78, // mr r4,r3
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
        0x80, 0x63, 0x00, 0x00, // lwz r3,0(r3)
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x40, 0x82, 0x00, 0x0c, // bne +12
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x48, 0x00, 0x00, 0x1c, // b +28
        0x48, 0x00, 0x00, 0x01, // bl is_updatable
        0x54, 0x60, 0x06, 0x3f, // clrlwi. r0,r3,24
        0x41, 0x82, 0x00, 0x0c, // beq +12
        0x38, 0x60, 0x00, 0x01, // li r3,1
        0x48, 0x00, 0x00, 0x08, // b +8
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    let call = object
        .windows(4)
        .position(|bytes| bytes == [0x48, 0x00, 0x00, 0x01])
        .expect("the external call should have a REL24 placeholder");
    let body_start = call.saturating_sub(36);
    let body_end = (call + 40).min(object.len());
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "unexpected guarded call body: {:02x?}",
        &object[body_start..body_end],
    );
}
