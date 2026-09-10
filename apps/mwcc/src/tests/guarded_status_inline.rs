use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn later_size_optimization_keeps_a_repeated_guarded_helper_out_of_line() {
    let object = compile(
        include_bytes!("../../../../canaries/2326_guarded_status_inline.c"),
        "guarded-status-inline.c",
        CompilerConfig {
            build: GC_3_0A3,
            flags: Flags {
                optimization: Optimization::O4,
                optimization_goal: OptimizationGoal::Size,
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
    let words: Vec<_> = function_bytes(&object, "direct")
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect();
    // GC/3.0a3 -O4,s retains classify's call and the caller's own guard.
    assert_eq!(
        words,
        [
            0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x7c7f1b78, 0x48000001, 0x2c03fff6,
            0x40820014, 0x801f0000, 0x5400077b, 0x41820008, 0x38600000, 0x80010014, 0x83e1000c,
            0x7c0803a6, 0x38210010, 0x4e800020,
        ]
    );
}
