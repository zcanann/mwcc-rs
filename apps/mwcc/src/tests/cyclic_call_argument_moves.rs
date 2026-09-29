use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};

fn compile_gc_2_6(source: &[u8]) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    compile(
        source,
        "cyclic-call-argument-moves.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the cyclic argument moves should compile")
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_be_bytes(word.try_into().unwrap()))
        .collect()
}

const PROLOGUE: [u32; 3] = [
    0x9421_fff0, // stwu r1,-16(r1)
    0x7c08_02a6, // mflr r0
    0x9001_0014, // stw r0,20(r1)
];
const EPILOGUE: [u32; 5] = [
    0x4800_0001, // bl k2 (REL24 placeholder)
    0x8001_0014, // lwz r0,20(r1)
    0x7c08_03a6, // mtlr r0
    0x3821_0010, // addi r1,r1,16
    0x4e80_0020, // blr
];

fn framed(body: &[u32]) -> Vec<u32> {
    PROLOGUE.iter().chain(body).chain(&EPILOGUE).copied().collect()
}

/// A computed argument that reads the ABI home of a later leaf argument must
/// run before that home is overwritten. mwcc saves the endangered incoming
/// value in r0 (after the LR save), computes, and then moves it back.
/// Exact GC/2.6 output measured from mwcceppc (also GC/1.3.2 and GC/2.7).
#[test]
fn saves_an_endangered_leaf_argument_in_r0_across_a_computed_argument() {
    let object = compile_gc_2_6(
        br#"
        int k2(int, int);
        int f12(int a, int b) { return k2(b - 1, a); }
        int f13(int a, int b) { return k2(b + a, a); }
        int f7(int a, int b) { return k2(b, a); }
    "#,
    );

    assert_eq!(
        words(function_bytes(&object, "f12")),
        framed(&[
            0x7c60_1b78, // mr r0,r3
            0x3864_ffff, // addi r3,r4,-1
            0x7c04_0378, // mr r4,r0
        ]),
    );
    assert_eq!(
        words(function_bytes(&object, "f13")),
        framed(&[
            0x7c60_1b78, // mr r0,r3
            0x7c64_0214, // add r3,r4,r0
            0x7c04_0378, // mr r4,r0
        ]),
    );
    assert_eq!(
        words(function_bytes(&object, "f7")),
        framed(&[
            0x7c60_1b78, // mr r0,r3
            0x7c83_2378, // mr r3,r4
            0x7c04_0378, // mr r4,r0
        ]),
    );
}
