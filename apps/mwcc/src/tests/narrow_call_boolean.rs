use crate::{compile, SourceLanguage};

#[test]
fn promotes_a_boolean_call_result_before_testing_it_for_nonzero() {
    let source = br#"
        struct Base;
        extern bool is_enabled(const Base*);

        unsigned enabled(void* object) {
            return is_enabled((Base*)object) != 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "narrow-call-boolean.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("a boolean call result comparison should compile");

    let expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x48, 0x00, 0x00, 0x01, // bl is_enabled
        0x54, 0x63, 0x06, 0x3e, // clrlwi r3,r3,24
        0x7c, 0x03, 0x00, 0xd0, // neg r0,r3
        0x7c, 0x00, 0x1b, 0x78, // or r0,r0,r3
        0x54, 0x03, 0x0f, 0xfe, // srwi r3,r0,31
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
