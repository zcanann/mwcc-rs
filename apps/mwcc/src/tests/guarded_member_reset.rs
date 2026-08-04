use crate::{compile, SourceLanguage};

#[test]
fn emits_the_melee_guarded_member_reset_schedule() {
    let source = br#"
        typedef unsigned char u8;

        typedef struct HSD_GObj HSD_GObj;
        typedef struct GalleryData {
            u8 state;
            u8 pending;
            u8 padding[6];
            HSD_GObj* object;
        } GalleryData;

        extern void stop_video(void);
        extern void stop_audio(void);
        extern int current_audio(void);
        extern void select_audio(int);
        extern void unlink_object(HSD_GObj*);

        void reset_gallery(void* argument) {
            GalleryData* data = argument;
            int zero;
            if (data->state != 0) {
                stop_video();
                stop_audio();
                select_audio(current_audio());
                zero = 0;
                data->state = zero;
                data->pending = zero;
                if (data->object != 0) {
                    unlink_object(data->object);
                    data->object = 0;
                }
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.ipa_file = true;
    let object = compile(
        source,
        "guarded-member-reset.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the guarded member reset should compile");

    // Exact GC/1.2.5n code measured from Melee's mnGallery_80258D50. The
    // schedule keeps the owner and named zero in r30/r31, reuses the guarded
    // pointer load as the cleanup argument, and reuses r31 for the final clear.
    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe8, 0x93, 0xe1,
        0x00, 0x14, 0x93, 0xc1, 0x00, 0x10, 0x7c, 0x7e, 0x1b, 0x78, 0x88, 0x03, 0x00, 0x00,
        0x28, 0x00, 0x00, 0x00, 0x41, 0x82, 0x00, 0x34, 0x48, 0x00, 0x00, 0x01, 0x48, 0x00,
        0x00, 0x01, 0x48, 0x00, 0x00, 0x01, 0x48, 0x00, 0x00, 0x01, 0x3b, 0xe0, 0x00, 0x00,
        0x9b, 0xfe, 0x00, 0x00, 0x9b, 0xfe, 0x00, 0x01, 0x80, 0x7e, 0x00, 0x08, 0x28, 0x03,
        0x00, 0x00, 0x41, 0x82, 0x00, 0x0c, 0x48, 0x00, 0x00, 0x01, 0x93, 0xfe, 0x00, 0x08,
        0x80, 0x01, 0x00, 0x1c, 0x83, 0xe1, 0x00, 0x14, 0x83, 0xc1, 0x00, 0x10, 0x38, 0x21,
        0x00, 0x18, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
