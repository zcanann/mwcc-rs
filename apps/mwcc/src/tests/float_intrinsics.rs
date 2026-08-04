use crate::{compile, SourceLanguage};

#[test]
fn narrows_a_widened_float_fabs_intrinsic_through_f0() {
    let source = br#"
        extern double __fabs(double);
        float fabsf(float value) {
            return (float)__fabs((double)value);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "float-intrinsics.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the widened fabs intrinsic should compile");

    let expected = [
        0xfc, 0x00, 0x0a, 0x10, // fabs f0,f1
        0xfc, 0x20, 0x00, 0x18, // frsp f1,f0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn lowers_an_implicitly_declared_double_fabs_intrinsic_without_a_call_frame() {
    let source = br#"
        double fabs_wrapper(double value) {
            return __fabs(value);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "float-intrinsics.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the implicit fabs intrinsic should compile as a leaf");

    let expected = [
        0xfc, 0x20, 0x0a, 0x10, // fabs f1,f1
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
