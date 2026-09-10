use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

#[test]
fn reproduces_the_patch_one_guard_parameter_overwriting_saved_r30() {
    let object = compile(
        br#"extern int query(int);
            int guarded(int x, int flag) {
                int r=query(x);
                if(flag && r<0) return r;
                return query(x+1);
            }"#,
        "shared-spill-guard.c",
        CompilerConfig {
            build: GC_1_1P1,
            flags: Flags {
                optimization: Optimization::O0,
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
    let words: Vec<_> = function_bytes(&object, "guarded")
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect();
    // Measured reference instructions, including stw r30,8(r1) followed by
    // stw r4,8(r1). The epilogue deliberately reloads the overwritten r30.
    assert_eq!(
        words,
        [
            0x7c0802a6, 0x90010004, 0x9421fff0, 0x93e1000c, 0x93c10008, 0x7c7e1b78, 0x90810008,
            0x7fc3f378, 0x48000001, 0x7c7f1b78, 0x80010008, 0x2c000000, 0x41820014, 0x2c1f0000,
            0x4080000c, 0x7fe3fb78, 0x4800000c, 0x387e0001, 0x48000001, 0x83e1000c, 0x83c10008,
            0x38210010, 0x80010004, 0x7c0803a6, 0x4e800020,
        ]
    );
}
