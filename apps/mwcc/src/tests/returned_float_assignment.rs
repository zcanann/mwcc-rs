use crate::{compile, SourceLanguage};

#[test]
fn folds_a_single_defined_returned_float_local() {
    let source = br#"
        typedef struct Vec { float x; float y; float z; } Vec;

        float square_magnitude(const Vec* value) {
            float result;
            (void)0;
            result = value->z * value->z
                + (value->x * value->x + value->y * value->y);
            return result;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.fp_contract = false;
    let object = compile(
        source,
        "returned-float-assignment.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a single-defined returned float local should fold into its value");

    // Exact GC/1.2.5n schedule measured from Dolphin's C_VECSquareMag.
    let expected = [
        0xc0, 0x23, 0x00, 0x00, // lfs f1,0(r3)
        0xc0, 0x03, 0x00, 0x04, // lfs f0,4(r3)
        0xec, 0x21, 0x00, 0x72, // fmuls f1,f1,f1
        0xc0, 0x43, 0x00, 0x08, // lfs f2,8(r3)
        0xec, 0x00, 0x00, 0x32, // fmuls f0,f0,f0
        0xec, 0x42, 0x00, 0xb2, // fmuls f2,f2,f2
        0xec, 0x01, 0x00, 0x2a, // fadds f0,f1,f0
        0xec, 0x22, 0x00, 0x2a, // fadds f1,f2,f0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
