use crate::{compile, SourceLanguage};

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|bytes| bytes == needle)
}

#[test]
fn lowers_unmaterialized_jgeometry_float_template_members_at_the_call_site() {
    let source = br#"
        typedef float f32;
        namespace JGeometry {
        template <typename T> struct TUtil {
            static const f32 epsilon();
            static f32 inv_sqrt(f32);
        };
        typedef TUtil<f32> TUtilf;
        }

        float compiled(float value) {
            return JGeometry::TUtilf::epsilon()
                 + JGeometry::TUtilf::inv_sqrt(value);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "jgeometry-float-utilities.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the JGeometry float utilities should lower without definitions");

    assert!(!contains_bytes(
        &object,
        b"epsilon__Q29JGeometry8TUtil<f>Fv\0"
    ));
    assert!(!contains_bytes(
        &object,
        b"inv_sqrt__Q29JGeometry8TUtil<f>Ff\0"
    ));
    assert!(contains_bytes(&object, b"__float_epsilon\0"));
    assert!(contains_bytes(
        &object,
        &[0xfc, 0x80, 0x08, 0x34] // frsqrte f4,f1
    ));
}
