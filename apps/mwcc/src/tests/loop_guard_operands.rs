use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};
use mwcc_versions::*;

fn config() -> CompilerConfig {
    CompilerConfig {
        build: GC_1_3,
        flags: Flags {
            optimization: Optimization::O4,
            debug_info: false,
            emit_mwcats: false,
            cpp_exceptions: false,
            ..Flags::default()
        },
    }
}

#[test]
fn preserves_a_member_base_through_a_global_mask_and_later_calls() {
    let object = compile(
        include_bytes!("../../../../canaries/2325_retained_member_global_mask.c"),
        "retained-member-mask.c",
        config(),
        Some(SourceLanguage::C),
        None,
        false,
    )
    .unwrap();
    let words: Vec<_> = function_bytes(&object, "retained_word")
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect();
    // Measured GC/1.3 O4 output keeps the entry pointer in r31 while the
    // loaded word and global mask occupy volatile registers.
    assert_eq!(
        words,
        [
            0x9421fff0, 0x7c0802a6, 0x90010014, 0x93e1000c, 0x7c7f1b78, 0x48000001, 0x2c030000,
            0x40800034, 0x809f0004, 0x80000000, 0x7c840038, 0x548003df, 0x41820010, 0x7fe3fb78,
            0x48000001, 0x48000014, 0x5480035b, 0x4182000c, 0x7fe3fb78, 0x48000001, 0x80010014,
            0x83e1000c, 0x7c0803a6, 0x38210010, 0x4e800020
        ]
    );
}

#[test]
fn compiles_condition_mutations_and_post_loop_guards() {
    // Every loop has mutable local state in its condition or body, followed
    // by a compact parser guard. These previously missed structured lowering.
    let object = compile(
        include_bytes!("../../../../canaries/2324_counted_byte_compare.c"),
        "counted-byte-compare.c",
        config(),
        Some(SourceLanguage::C),
        None,
        false,
    )
    .unwrap();
    for name in [
        "compare32",
        "compare_dynamic",
        "guard_count",
        "guard_byte",
        "guard_cursor",
    ] {
        assert!(!function_bytes(&object, name).is_empty(), "{name}");
    }
}
