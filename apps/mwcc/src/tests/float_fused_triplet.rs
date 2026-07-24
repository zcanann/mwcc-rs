use crate::{compile, SourceLanguage};

#[test]
fn contracts_three_float_member_loads_into_mwccs_fused_triplet() {
    let source = br#"
        struct Values { float addend, multiplicand, pad, multiplier; };
        void compiled(struct Values* values) {
            values->addend += values->multiplicand * values->multiplier;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "float-fused-triplet.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("three located float operands should contract");

    let expected = [
        0xc0, 0x43, 0x00, 0x04, // lfs f2,4(r3)
        0xc0, 0x23, 0x00, 0x0c, // lfs f1,12(r3)
        0xc0, 0x03, 0x00, 0x00, // lfs f0,0(r3)
        0xec, 0x02, 0x00, 0x7a, // fmadds f0,f2,f1,f0
        0xd0, 0x03, 0x00, 0x00, // stfs f0,0(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
