use crate::{compile, SourceLanguage};

#[test]
fn selects_a_power_of_two_from_a_cleared_low_bit_across_generations() {
    let source = br#"
        int select_even_nibble(int value) {
            return ((value & 1) == 0) ? 4 : 0;
        }
    "#;

    // Exact representative bodies measured from mwcceppc. The transitions are
    // 2.3.3 branch diamond -> 2.4.x materialized AND -> 4.x bit extraction.
    let generations: [(mwcc_versions::CompilerBuild, &[u8]); 3] = [
        (
            mwcc_versions::GC_1_2_5,
            &[
                0x54, 0x60, 0x07, 0xff, 0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x04, 0x4e,
                0x80, 0x00, 0x20, 0x38, 0x60, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
            ],
        ),
        (
            mwcc_versions::GC_1_3,
            &[
                0x54, 0x63, 0x07, 0xfe, 0x38, 0x00, 0x00, 0x04, 0x38, 0x63, 0xff, 0xff, 0x7c,
                0x03, 0x18, 0x38, 0x4e, 0x80, 0x00, 0x20,
            ],
        ),
        (
            mwcc_versions::GC_3_0A3,
            &[
                0x54, 0x63, 0x07, 0xfe, 0x38, 0x03, 0xff, 0xff, 0x54, 0x03, 0x07, 0x7a, 0x4e,
                0x80, 0x00, 0x20,
            ],
        ),
    ];

    for (build, expected) in generations {
        let mut flags = mwcc_versions::Flags::default();
        flags.debug_info = false;
        flags.emit_mwcats = false;
        flags.inline_enabled = false;
        let object = compile(
            source,
            "cleared-low-bit-power-select.c",
            mwcc_versions::CompilerConfig { build, flags },
            Some(SourceLanguage::C),
            None,
            false,
        )
        .expect("cleared-low-bit power select should compile");
        assert!(
            object
                .windows(expected.len())
                .any(|bytes| bytes == expected),
            "missing exact {} cleared-low-bit power select body",
            build.label,
        );
    }
}
