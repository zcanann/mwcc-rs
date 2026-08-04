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

    // GC/1.3--2.7 finish the call-result chain while f31 is still live, issue
    // the paired lane, overlap the saved-LR load with the final double lane,
    // then write LR. This is the complete oracle tail for GC/2.0p1.
    let expected_tail = [
        0xec, 0x21, 0xf8, 0x2a, // fadds f1,f1,f31
        0xe3, 0xe1, 0x00, 0x18, // psq_l f31,24(r1),0,0
        0x80, 0x01, 0x00, 0x24, // lwz r0,36(r1)
        0xcb, 0xe1, 0x00, 0x10, // lfd f31,16(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x20, // addi r1,r1,32
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected_tail.len())
        .any(|bytes| bytes == expected_tail));
}
