use crate::{compile, SourceLanguage};

#[test]
fn widens_signed_and_unsigned_narrow_leaves_before_float_arguments() {
    let source = br#"
        extern void consume(float);

        void unsigned_value(unsigned char value) {
            consume((float) value);
        }

        void signed_value(signed char value) {
            consume((float) value);
        }
    "#;
    let object = compile(
        source,
        "narrow-integer-to-float-argument.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags: mwcc_versions::Flags::default(),
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("narrow register leaves should widen before floating conversion");

    assert!(object
        .windows(4)
        .any(|bytes| bytes == [0x54, 0x60, 0x06, 0x3e])); // clrlwi r0,r3,24
    assert!(object
        .windows(4)
        .any(|bytes| bytes == [0x7c, 0x60, 0x07, 0x74])); // extsb r0,r3
}
