use crate::{compile, SourceLanguage};

#[test]
fn preserves_the_old_value_of_a_frame_resident_postfix_step() {
    let source = br#"
        int frame_post_increment(int value) {
            volatile int local = value;
            return local++;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "frame-post-step.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a frame-resident postfix value should compile");

    // Exact GC/2.6 value-producing core measured from mwcceppc: return the
    // loaded old value in r3 while incrementing and writing back through r0.
    let expected = [
        0x80, 0x61, 0x00, 0x08, 0x38, 0x03, 0x00, 0x01, 0x90, 0x01, 0x00, 0x08,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact frame postfix load/add/store sequence"
    );
}
