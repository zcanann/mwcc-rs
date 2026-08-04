use crate::{compile, SourceLanguage};

#[test]
fn fuses_adjacent_single_and_double_interpolations_exactly() {
    let source = br#"
        float interpolate(float amount, float* values, unsigned index) {
            return (1.0f - amount) * values[index] + amount * values[index + 1];
        }
        double interpolate_double(double amount, double* values, unsigned index) {
            return (1.0 - amount) * values[index] + amount * values[index + 1];
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "adjacent-linear-interpolation.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("adjacent interpolations should compile");

    let single = [
        0x54, 0x80, 0x10, 0x3a, 0xc0, 0x60, 0x00, 0x00, 0x7c, 0x83, 0x02, 0x14, 0x7c, 0x43,
        0x04, 0x2e, 0xc0, 0x04, 0x00, 0x04, 0xec, 0x63, 0x08, 0x28, 0xec, 0x01, 0x00, 0x32,
        0xec, 0x23, 0x00, 0xba, 0x4e, 0x80, 0x00, 0x20,
    ];
    let double = [
        0x54, 0x80, 0x18, 0x38, 0xc8, 0x60, 0x00, 0x00, 0x7c, 0x83, 0x02, 0x14, 0x7c, 0x43,
        0x04, 0xae, 0xc8, 0x04, 0x00, 0x08, 0xfc, 0x63, 0x08, 0x28, 0xfc, 0x01, 0x00, 0x32,
        0xfc, 0x23, 0x00, 0xba, 0x4e, 0x80, 0x00, 0x20,
    ];
    for (name, expected) in [("single", &single[..]), ("double", &double[..])] {
        assert!(
            object.windows(expected.len()).any(|bytes| bytes == expected),
            "missing exact {name} interpolation",
        );
    }
}
