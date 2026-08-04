use crate::{compile, SourceLanguage};

#[test]
fn preserves_lr_and_reuses_a_member_across_an_early_return_guard() {
    let source = br#"
        struct Channel { int padding[8]; struct Voice* voice; };
        struct Voice { int state[4]; };
        extern void stop(struct Voice* voice);

        int force_stop(struct Channel* channel) {
            if (channel->voice == 0) {
                return 0;
            }
            stop(channel->voice);
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "guarded-member-early-return.c",
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
        0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
        0x80, 0x63, 0x00, 0x20, // lwz r3,32(r3)
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x40, 0x82, 0x00, 0x0c, // bne +12
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x48, 0x00, 0x00, 0x0c, // b +12
        0x48, 0x00, 0x00, 0x01, // bl stop
        0x38, 0x60, 0x00, 0x01, // li r3,1
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "guarded member early-return body was not found in object: {:02x?}",
        object
    );
}
