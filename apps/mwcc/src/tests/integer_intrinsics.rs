use crate::{compile, SourceLanguage};

#[test]
fn lowers_integer_abs_intrinsics_as_exact_leaf_instructions() {
    let source = br#"
        int leaf_abs(int value) {
            return __abs(value);
        }
        int nested_abs(int left, int right) {
            return right + __abs(left - right);
        }
        int computed_abs(int left, int right) {
            return __abs(left - right);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "integer-intrinsics.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the integer absolute-value intrinsic should compile");

    // Exact GC/3.0a3 .text measured from mwcceppc. The three functions are
    // contiguous, so this also verifies that none acquires a call frame.
    let expected = [
        0x7c, 0x60, 0xfe, 0x70, // srawi r0,r3,31
        0x7c, 0x03, 0x1a, 0x78, // xor r3,r0,r3
        0x7c, 0x60, 0x18, 0x50, // subf r3,r0,r3
        0x4e, 0x80, 0x00, 0x20, // blr
        0x7c, 0x04, 0x18, 0x50, // subf r0,r4,r3
        0x7c, 0x03, 0xfe, 0x70, // srawi r3,r0,31
        0x7c, 0x60, 0x02, 0x78, // xor r0,r3,r0
        0x7c, 0x03, 0x00, 0x50, // subf r0,r3,r0
        0x7c, 0x64, 0x02, 0x14, // add r3,r4,r0
        0x4e, 0x80, 0x00, 0x20, // blr
        0x7c, 0x64, 0x18, 0x50, // subf r3,r4,r3
        0x7c, 0x60, 0xfe, 0x70, // srawi r0,r3,31
        0x7c, 0x03, 0x1a, 0x78, // xor r3,r0,r3
        0x7c, 0x60, 0x18, 0x50, // subf r3,r0,r3
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
    assert!(!object.windows(6).any(|bytes| bytes == b"__abs\0"));
}
