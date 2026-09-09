use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn matches_reference_pair_survival_and_versioned_teardown() {
    // Full bodies measured from the listed mwcceppc builds. Calls remain relocatable.
    let cases: &[(CompilerBuild, Optimization, &[u32])] = &[
        (
            GC_1_1,
            Optimization::O4,
            &[
                0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x48000001, 0x3bc40000,
                0x3be30000, 0x48000001, 0x389e0000, 0x387f0000, 0x48000001, 0x80010014, 0x83e1000c,
                0x83c10008, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        ),
        (
            GC_1_1,
            Optimization::O3,
            &[
                0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x48000001, 0x3bc40000,
                0x3be30000, 0x48000001, 0x389e0000, 0x387f0000, 0x48000001, 0x80010014, 0x83e1000c,
                0x83c10008, 0x38210010, 0x7c0803a6, 0x4e800020,
            ],
        ),
        (
            GC_1_1P1,
            Optimization::O4,
            &[
                0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x48000001, 0x3bc40000,
                0x3be30000, 0x48000001, 0x389e0000, 0x387f0000, 0x48000001, 0x83e1000c, 0x83c10008,
                0x38210010, 0x80010004, 0x7c0803a6, 0x4e800020,
            ],
        ),
        (
            GC_1_2_5,
            Optimization::O4,
            &[
                0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x48000001, 0x3bc40000,
                0x3be30000, 0x48000001, 0x389e0000, 0x387f0000, 0x48000001, 0x80010014, 0x83e1000c,
                0x83c10008, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        ),
        (
            GC_1_2_5N,
            Optimization::O4,
            &[
                0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x48000001, 0x3bc40000,
                0x3be30000, 0x48000001, 0x389e0000, 0x387f0000, 0x48000001, 0x80010014, 0x83e1000c,
                0x83c10008, 0x38210010, 0x7c0803a6, 0x4e800020,
            ],
        ),
        (
            GC_1_3,
            Optimization::O4,
            &[
                0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x93c10008, 0x48000001, 0x7c9e2378,
                0x7c7f1b78, 0x48000001, 0x7fc4f378, 0x7fe3fb78, 0x48000001, 0x80010014, 0x83e1000c,
                0x83c10008, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        ),
        (
            GC_1_3,
            Optimization::O3,
            &[
                0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x93c10008, 0x48000001, 0x7c9e2378,
                0x7c7f1b78, 0x48000001, 0x7fc4f378, 0x7fe3fb78, 0x48000001, 0x83e1000c, 0x83c10008,
                0x80010014, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        ),
        (
            GC_1_3,
            Optimization::O0,
            &[
                0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x93c10008, 0x48000001, 0x7c9f2378,
                0x7c7e1b78, 0x48000001, 0x7fe4fb78, 0x7fc3f378, 0x48000001, 0x83e1000c, 0x83c10008,
                0x80010014, 0x7c0803a6, 0x38210010, 0x4e800020,
            ],
        ),
    ];
    for &(build, optimization, expected) in cases {
        let flags = Flags {
            optimization,
            debug_info: false,
            emit_mwcats: false,
            inline_enabled: false,
            cpp_exceptions: false,
            ..Flags::default()
        };
        let object = compile(
            br#"
            typedef unsigned long long u64;
            extern u64 first_value(void);
            extern void disturb(void);
            extern void consume(u64);
            void across_call(void) { u64 value = first_value(); disturb(); consume(value); }
        "#,
            "wide-pairs.c",
            CompilerConfig { build, flags },
            Some(SourceLanguage::C),
            None,
            false,
        )
        .unwrap();
        let actual: Vec<_> = function_bytes(&object, "across_call")
            .chunks_exact(4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(actual, expected, "{} {:?}", build.label, optimization);
    }
}
