use crate::{compile, SourceLanguage};

#[test]
fn extracts_power_of_two_member_masks_as_booleans() {
    let source = br#"
        struct Probe {
            unsigned int flags;
        };

        int masked_member_truth(struct Probe* probe) {
            return (probe->flags & 8) != 0;
        }

        int masked_member_select(struct Probe* probe) {
            return (probe->flags & 4) ? 1 : 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "masked-member-boolean.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("masked member booleans should compile");

    // Exact GC/2.6 code measured from mwcceppc.
    let expected = [
        0x80, 0x03, 0x00, 0x00, 0x54, 0x03, 0xef, 0xfe, 0x4e, 0x80, 0x00, 0x20, 0x80, 0x03,
        0x00, 0x00, 0x54, 0x03, 0xf7, 0xfe, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact masked-member boolean bodies"
    );
}
