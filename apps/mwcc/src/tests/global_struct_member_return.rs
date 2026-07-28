use crate::{compile, SourceLanguage};

#[test]
fn materializes_an_offset_zero_global_struct_member_return_address() {
    let source = br#"
        struct SoundGlobals {
            unsigned stereo;
            unsigned count;
            float category_volume[5];
            unsigned char voices[4096];
        };

        extern struct SoundGlobals sound_globals;

        unsigned sound_mode(void* context) {
            return sound_globals.stereo;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.global_addressing = mwcc_versions::GlobalAddressing::Absolute;
    let object = compile(
        source,
        "global-struct-member-return.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an offset-zero global struct member return should compile");

    let expected = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,sound_globals@ha
        0x38, 0x63, 0x00, 0x00, // addi r3,r3,sound_globals@l
        0x80, 0x63, 0x00, 0x00, // lwz r3,0(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
