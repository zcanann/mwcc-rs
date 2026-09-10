use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn matches_the_card_record_stride_load() {
    // CARDControl is 272 bytes and xferred is at offset 184. The reference
    // scales before reusing the channel's register for the global base.
    for build in SUPPORTED.iter().chain(EXPERIMENTAL).copied() {
        let object = compile(
            br#"struct Card { unsigned char prefix[184]; int xferred;
                              unsigned char suffix[84]; };
                extern struct Card cards[4];
                int transferred(int channel) { return cards[channel].xferred; }"#,
            "card-stride.c",
            CompilerConfig {
                build,
                flags: Flags {
                    optimization: Optimization::O4,
                    debug_info: false,
                    emit_mwcats: false,
                    inline_enabled: false,
                    cpp_exceptions: false,
                    ..Flags::default()
                },
            },
            Some(SourceLanguage::C),
            None,
            false,
        )
        .unwrap();
        let actual: Vec<_> = function_bytes(&object, "transferred")
            .chunks_exact(4)
            .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(
            actual,
            [0x1c030110, 0x3c600000, 0x38630000, 0x7c630214, 0x806300b8, 0x4e800020],
            "{}",
            build.label
        );
    }
}
