use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn matches_reference_comparisons_across_the_immediate_boundary() {
    let object = compile(
        include_bytes!("../../../../canaries/2322_narrow_comparison_constants.c"),
        "narrow-comparison-constants.c",
        CompilerConfig {
            build: GC_1_3,
            flags: Flags {
                optimization: Optimization::O4,
                debug_info: false,
                emit_mwcats: false,
                cpp_exceptions: false,
                ..Flags::default()
            },
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .unwrap();
    // Measured GC/1.3 O4 instruction words. The same 0xffff low bits need
    // unsigned-immediate, signed-immediate, or full-word comparison depending
    // on integer promotion and the constant's type/value.
    let fixtures: &[(&str, &[u32])] = &[
        (
            "half_equal",
            &[
                0x5460043e, 0x2800ffff, 0x4c820020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
        (
            "half_negative",
            &[
                0x5460043e, 0x2c00ffff, 0x4c820020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
        (
            "word_negative",
            &[
                0x3c030001, 0x2800ffff, 0x4c820020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
        (
            "word_negative_less",
            &[
                0x3800ffff, 0x7c030040, 0x4c800020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
        (
            "signed_large",
            &[
                0x7c630734, 0x3c030000, 0x2800ffff, 0x4c820020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
        (
            "signed_limit",
            &[
                0x3ca00001, 0x7c630734, 0x3805ffff, 0x7c030000, 0x4c800020, 0x38000001, 0x90040000,
                0x4e800020,
            ],
        ),
        (
            "signed_negative",
            &[
                0x3ca0ffff, 0x7c630734, 0x38057fff, 0x7c030000, 0x4c800020, 0x38000001, 0x90040000,
                0x4e800020,
            ],
        ),
        (
            "half_left",
            &[
                0x5460043e, 0x2800ffff, 0x4c800020, 0x38000001, 0x90040000, 0x4e800020,
            ],
        ),
    ];
    for (name, expected) in fixtures {
        let words: Vec<_> = function_bytes(&object, name)
            .chunks_exact(4)
            .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(&words, expected, "{name}");
    }
}
