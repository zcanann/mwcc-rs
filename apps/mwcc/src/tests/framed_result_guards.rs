use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn call_result_guards_return_through_the_frame_epilogue() {
    // The early result is still in r3, but LR points immediately after query.
    // A conditional bclr here loops inside the function instead of returning.
    for build in SUPPORTED.iter().chain(EXPERIMENTAL).copied() {
        for optimization in [Optimization::O0, Optimization::O4] {
            let object = compile(
                br#"extern int query(int);
                    int guarded(int x) {
                        int result = query(x);
                        if (result < 0) return result;
                        return query(x + 1);
                    }
                    int twice(int x) {
                        int result = query(x);
                        if (result < 0) return result;
                        result = query(x + 1);
                        if (result < 0) return result;
                        return x;
                    }"#,
                "framed-result-guard.c",
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
            for name in ["guarded", "twice"] {
                let words: Vec<_> = function_bytes(&object, name)
                    .chunks_exact(4)
                    .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
                    .collect();
                assert!(words.iter().any(|w| w & 0xffff0000 == 0x94210000));
                assert!(words.iter().any(|w| w >> 26 == 18 && w & 1 != 0));
                assert!(
                    !words
                        .iter()
                        .any(|w| w & 0xfc0007fe == 0x4c000020 && (w >> 21) & 31 != 20),
                    "{} {:?}: conditional return bypasses the frame epilogue",
                    build.label,
                    optimization
                );
                if name == "twice" {
                    for (index, instruction) in
                        words.iter().enumerate().filter(|(_, w)| *w >> 26 == 16)
                    {
                        let displacement = (*instruction & 0xfffc) as u16 as i16 as isize / 4;
                        let target = (index as isize + displacement) as usize;
                        let tail = &words[target..];
                        assert!(
                            tail.iter().any(|w| w & 0xffff0000 == 0x80010000),
                            "{} {:?}: early return skips the saved LR reload",
                            build.label,
                            optimization
                        );
                        assert!(
                            !tail.iter().any(|w| w & 0xfc1f07ff == 0x7c030378),
                            "{} {:?}: early result overwritten by the fallthrough result",
                            build.label,
                            optimization
                        );
                    }
                }
            }
        }
    }
}
