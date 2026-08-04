use crate::{compile, SourceLanguage};

#[test]
fn retains_one_zero_across_a_saved_receiver_cleanup() {
    let source = br#"
        struct Voice {
            unsigned char index;
            unsigned char pad[5];
            unsigned short timer;
            int pad2;
            void* callback;
        };
        struct Channel { int pad[8]; struct Voice* voice; };
        extern void play_stop(unsigned char index);
        extern void flush(unsigned char index);
        extern void release(struct Voice* voice, unsigned int owner);

        int stop_channel(struct Channel* channel) {
            struct Voice* voice = channel->voice;
            if (voice == 0) {
                return 0;
            }
            voice->callback = 0;
            channel->voice->timer = 0;
            play_stop(channel->voice->index);
            flush(channel->voice->index);
            release(channel->voice, (unsigned int)channel);
            channel->voice = 0;
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.use_lmw_stmw = true;
    let object = compile(
        source,
        "retained-zero-cleanup.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the retained-zero cleanup should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe8, 0xbf, 0xc1,
        0x00, 0x10, 0x7c, 0x7e, 0x1b, 0x78, 0x80, 0x63, 0x00, 0x20, 0x28, 0x03, 0x00, 0x00,
        0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00, 0x00, 0x40, 0x3b, 0xe0,
        0x00, 0x00, 0x93, 0xe3, 0x00, 0x0c, 0x80, 0x7e, 0x00, 0x20, 0xb3, 0xe3, 0x00, 0x06,
        0x80, 0x7e, 0x00, 0x20, 0x88, 0x63, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01, 0x80, 0x7e,
        0x00, 0x20, 0x88, 0x63, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01, 0x80, 0x7e, 0x00, 0x20,
        0x7f, 0xc4, 0xf3, 0x78, 0x48, 0x00, 0x00, 0x01, 0x93, 0xfe, 0x00, 0x20, 0x38, 0x60,
        0x00, 0x01, 0x80, 0x01, 0x00, 0x1c, 0xbb, 0xc1, 0x00, 0x10, 0x38, 0x21, 0x00, 0x18,
        0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object.windows(expected.len()).any(|bytes| bytes == expected),
        "retained-zero cleanup body was not found in object: {:02x?}",
        object
    );
}
