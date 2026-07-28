use crate::{compile, SourceLanguage};

#[test]
fn converts_a_scaled_global_float_member_through_the_unsigned_runtime() {
    let source = br#"
        struct SoundState {
            int first;
            int second;
            float volume[4];
        };
        extern struct SoundState sound;

        unsigned volume(void* context) {
            double bias = 0.5;
            double scale = 10.0;
            double value = sound.volume[2];
            value *= scale;
            value = bias + value;
            return value;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "float-to-unsigned-runtime.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the float-to-unsigned runtime conversion should compile");

    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x60, 0x00, 0x00, 0x90, 0x01, 0x00,
        0x14, 0x38, 0x63, 0x00, 0x00, 0xc8, 0x20, 0x00, 0x00, 0xc0, 0x43, 0x00, 0x10, 0xc8, 0x00,
        0x00, 0x00, 0xfc, 0x42, 0x00, 0x72, 0xfc, 0x20, 0x10, 0x2a, 0x48, 0x00, 0x00, 0x01, 0x80,
        0x01, 0x00, 0x14, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
