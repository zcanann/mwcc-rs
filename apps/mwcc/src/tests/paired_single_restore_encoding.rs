use crate::{compile, SourceLanguage};

#[test]
fn restores_a_saved_fpr_with_one_displacement_form_psq_load() {
    let mut flags = mwcc_versions::Flags::default();
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        br#"
            extern float transform(float);
            float retain(float value, float addend) {
                float transformed = transform(value);
                return transformed + addend;
            }
        "#,
        "paired-single-restore-encoding.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the saved floating parameter should compile");

    // Every tested GC/1.3.2--2.7 and Wii/1.0--1.7 oracle uses the one-word
    // `psq_l f31,24(r1),0,0` form for this encodable lane. The old lowering
    // materialized 24 in r0 and emitted `psq_lx f31,r1,r0,0,0` instead.
    assert_eq!(
        object
            .windows(4)
            .filter(|bytes| *bytes == [0xe3, 0xe1, 0x00, 0x18])
            .count(),
        1
    );
    assert!(!object
        .windows(8)
        .any(|bytes| bytes == [0x38, 0x00, 0x00, 0x18, 0x13, 0xe1, 0x00, 0x0c]));
}
