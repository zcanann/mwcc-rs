use crate::{compile, SourceLanguage};

#[test]
fn contracts_promoted_integer_products_in_both_subtraction_directions() {
    let source = br#"
        float base_minus_product(float base, float scale, int count) {
            return base - count * scale;
        }
        float product_minus_base(float base, float scale, int count) {
            return count * scale - base;
        }
        double double_base_minus_product(double base, double scale, int count) {
            return base - count * scale;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "mixed-integer-float-fusion.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("mixed integer/float products should compile");

    for (name, fused_opcode) in [
        ("single base minus product", [0xec, 0x20, 0x08, 0xbc]),
        ("single product minus base", [0xec, 0x20, 0x08, 0xb8]),
        ("double base minus product", [0xfc, 0x20, 0x08, 0xbc]),
    ] {
        assert!(
            object.windows(fused_opcode.len()).any(|bytes| bytes == fused_opcode),
            "missing {name} fused instruction",
        );
    }
}
