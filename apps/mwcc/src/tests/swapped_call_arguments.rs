use crate::{compile, SourceLanguage};

#[test]
fn preserves_both_sides_of_a_two_register_call_argument_swap() {
    let source = br#"
        struct Manager;
        struct Context {
            struct Manager* manager;
        };

        void add_client(struct Manager* manager, struct Context* context);

        void init(struct Context* context, struct Manager* manager) {
            context->manager = manager;
            add_client(manager, context);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "swapped-call-arguments.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the swapped call arguments should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x7c, 0x65, 0x1b, 0x78, // mr r5,r3
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
        0x90, 0x83, 0x00, 0x00, // stw r4,0(r3)
        0x38, 0x64, 0x00, 0x00, // addi r3,r4,0
        0x38, 0x85, 0x00, 0x00, // addi r4,r5,0
        0x48, 0x00, 0x00, 0x01, // bl add_client
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    let call = object
        .windows(4)
        .position(|bytes| bytes == [0x48, 0x00, 0x00, 0x01])
        .expect("the external call should have a REL24 placeholder");
    let body_start = call.saturating_sub(28);
    let body_end = (call + 24).min(object.len());
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "unexpected call body: {:02x?}",
        &object[body_start..body_end],
    );
}
