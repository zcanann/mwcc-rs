use crate::{compile, SourceLanguage};

#[test]
fn evaluates_the_right_computed_subtrahend_first() {
    let source = br#"
        int computed_subtract(int a, int b, int c, int d) {
            return (a - b) - (c - d);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "computed-subtract.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("two computed subtraction operands should lower");

    // Exact GC/2.6 code measured from mwcceppc.
    let expected = [
        0x7c, 0xa6, 0x28, 0x50, 0x7c, 0x04, 0x18, 0x50, 0x7c, 0x65, 0x00, 0x50, 0x4e, 0x80,
        0x00, 0x20,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact computed-subtract body"
    );
}
