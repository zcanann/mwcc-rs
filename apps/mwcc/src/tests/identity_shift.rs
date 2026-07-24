use crate::{compile, SourceLanguage};

#[test]
fn folds_a_zero_shift_before_operand_placement() {
    let source = br#"
        unsigned identity_shift(unsigned value) {
            return (value & 0x00ffffffu) << 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "identity-shift.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a zero shift should fold to its left operand");

    // Exact GC/2.6 output measured from mwcceppc.
    let expected = [0x54, 0x63, 0x02, 0x3e, 0x4e, 0x80, 0x00, 0x20];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
