use crate::{compile, SourceLanguage};

#[test]
fn converts_a_computed_comparison_to_a_float_call_argument_without_clobbering_its_image() {
    let source = br#"
        extern void consume(float);

        void compiled(unsigned char value) {
            consume((float) (value == 1));
        }
    "#;
    let object = compile(
        source,
        "comparison-to-float-argument.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags: mwcc_versions::Flags::default(),
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a computed comparison should convert to a floating call argument");

    // The boolean remains in r0 through xoris and the low-word store. The
    // 0x4330 high word therefore needs an independent register until its store;
    // reusing a prematurely hoisted r0 value corrupts the conversion image.
    let safe_image = [
        0x54, 0x00, 0xd9, 0x7e, // srwi r0,r0,5
        0x6c, 0x00, 0x80, 0x00, // xoris r0,r0,0x8000
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x3c, 0x60, 0x43, 0x30, // lis r3,0x4330
    ];
    assert!(object
        .windows(safe_image.len())
        .any(|bytes| bytes == safe_image));
    assert!(object
        .windows(4)
        .any(|bytes| bytes == [0x90, 0x61, 0x00, 0x10])); // stw r3,16(r1)
}
