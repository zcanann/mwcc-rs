use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn matches_identity_division_and_unoptimized_negative_one() {
    // Measured in all 15 reference builds at O0 and O4. O0 retains divw for
    // signed /-1; optimized builds select neg. Both signed and unsigned /1
    // disappear in both modes.
    for build in SUPPORTED.iter().chain(EXPERIMENTAL).copied() {
        for optimization in [Optimization::O0, Optimization::O4] {
            let object = compile(
                br#"
                    int identity(int n) { return n/1; }
                    int negate(int n) { return n/-1; }
                    unsigned unsigned_identity(unsigned n) { return n/1; }
                "#,
                "division-identities.c",
                CompilerConfig {
                    build,
                    flags: Flags {
                        optimization,
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
            for name in ["identity", "unsigned_identity"] {
                assert_eq!(
                    function_bytes(&object, name),
                    0x4e800020u32.to_be_bytes(),
                    "{} {:?}: {}",
                    build.label,
                    optimization,
                    name
                );
            }
            let expected: &[u32] = if optimization == Optimization::O0 {
                &[0x3800ffff, 0x7c6303d6, 0x4e800020]
            } else {
                &[0x7c6300d0, 0x4e800020]
            };
            let actual: Vec<_> = function_bytes(&object, "negate")
                .chunks_exact(4)
                .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(actual, expected, "{} {:?}", build.label, optimization);
        }
    }
}
