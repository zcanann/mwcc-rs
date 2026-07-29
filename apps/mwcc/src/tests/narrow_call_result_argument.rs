use crate::{compile, SourceLanguage};

#[test]
fn forwards_a_narrow_call_result_to_a_narrow_parameter_without_extension() {
    let source = br#"
        typedef signed char s8;
        typedef unsigned char u8;
        extern u8 inner(void);
        extern int outer(s8);

        int f(void) {
            return outer(inner());
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "narrow-call-result-argument.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the narrow result should forward directly through r3");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
        0x48, 0x00, 0x00, 0x01, // bl inner
        0x48, 0x00, 0x00, 0x01, // bl outer
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
