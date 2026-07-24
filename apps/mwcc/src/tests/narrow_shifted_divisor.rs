use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_narrow_shifted_member_as_a_register_divisor() {
    let source = br#"
        typedef unsigned short u16;
        typedef unsigned int u32;

        struct Probe {
            int padding;
            u16 width;
        };

        u32 scaled_reciprocal(struct Probe* probe) {
            return (1 << 12) / (u32)((u16)probe->width << 1);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "narrow-shifted-divisor.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the register divide should allocate both operands");

    // Exact GC/2.6 code measured from mwcceppc.
    let expected = [
        0xa0, 0x03, 0x00, 0x04, 0x38, 0x60, 0x10, 0x00, 0x54, 0x00, 0x0b, 0xfc, 0x7c, 0x63,
        0x03, 0x96, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "missing exact narrow shifted-divisor body"
    );
}
