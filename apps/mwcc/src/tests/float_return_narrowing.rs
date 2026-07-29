use crate::{compile, SourceLanguage};

#[test]
fn structured_call_return_owns_one_double_to_float_narrowing() {
    let source = br#"
        extern double wrapped(double, double);

        float wrapper(float left, float right) {
            return wrapped(left, right);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "float-return-narrowing.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the double-valued call returned as float should compile");

    let expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x48, 0x00, 0x00, 0x01, // bl wrapped
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0xfc, 0x20, 0x08, 0x18, // frsp f1,f1
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "the structured return must narrow exactly once before linkage teardown",
    );
}
