use crate::{compile, SourceLanguage};

#[test]
fn retains_the_legacy_entry_lane_for_a_post_call_member_chain() {
    let source = br#"
        struct DspChannel { unsigned char index; };
        struct Channel { int padding[8]; struct DspChannel* dsp; };
        void update(struct Channel* channel);
        void flush(unsigned char index);

        void run(struct Channel* channel) {
            update(channel);
            flush(channel->dsp->index);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "post-call-member-parameter-frame.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the saved parameter member chain should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xe8, // stwu r1,-24(r1)
        0x93, 0xe1, 0x00, 0x14, // stw r31,20(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0x48, 0x00, 0x00, 0x01, // bl update
        0x80, 0x7f, 0x00, 0x20, // lwz r3,32(r31)
        0x88, 0x63, 0x00, 0x00, // lbz r3,0(r3)
        0x48, 0x00, 0x00, 0x01, // bl flush
        0x80, 0x01, 0x00, 0x1c, // lwz r0,28(r1)
        0x83, 0xe1, 0x00, 0x14, // lwz r31,20(r1)
        0x38, 0x21, 0x00, 0x18, // addi r1,r1,24
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
