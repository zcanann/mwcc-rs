use crate::{compile, SourceLanguage};

#[test]
fn records_a_promoted_narrow_member_sum_compared_with_zero() {
    let source = br#"
        struct Bounds {
            unsigned short width;
        };

        void retain_positive_width(struct Bounds* bounds, int delta, int* output) {
            if ((bounds->width + delta) <= 0) {
                return;
            }
            *output = 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "computed-record-condition.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a promoted narrow-member sum should compile");

    // Exact GC/2.6 code measured from mwcceppc. The unsigned-short member
    // promotes to signed int, and the final add records CR0 for the `blelr`.
    let expected = [
        0xa0, 0x03, 0x00, 0x00, 0x7c, 0x00, 0x22, 0x15, 0x4c, 0x81, 0x00, 0x20, 0x38, 0x00, 0x00,
        0x01, 0x90, 0x05, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact promoted-sum condition body"
    );
}
