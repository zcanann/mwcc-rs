use crate::{compile, SourceLanguage};

#[test]
fn converts_an_integer_call_argument_to_float_in_a_disjoint_frame_image() {
    let source = br#"
        extern void consume_float(float value);

        void pass_as_float(int value) {
            consume_float(value);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "implicit-int-to-float-argument.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a nonconstant integer argument should convert to float");

    // Exact GC/2.6 code measured from mwcceppc. Relocations leave the pooled
    // bias load and branch immediates as zero in the relocatable object.
    let expected = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x6c, 0x63, 0x80, 0x00, 0xc8, 0x20, 0x00,
        0x00, 0x90, 0x01, 0x00, 0x14, 0x3c, 0x00, 0x43, 0x30, 0x90, 0x61, 0x00, 0x0c, 0x90, 0x01,
        0x00, 0x08, 0xc8, 0x01, 0x00, 0x08, 0xec, 0x20, 0x08, 0x28, 0x48, 0x00, 0x00, 0x01, 0x80,
        0x01, 0x00, 0x14, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact implicit int-to-float argument body"
    );
}
