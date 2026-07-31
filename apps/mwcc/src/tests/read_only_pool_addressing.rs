use crate::{compile, SourceLanguage};

#[test]
fn sdata2_zero_uses_an_absolute_constant_pool_load() {
    let source = br#"
        float scale(float value) {
            return value * 2.0f;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.read_only_global_addressing = mwcc_versions::GlobalAddressing::Absolute;
    let object = compile(
        source,
        "absolute-read-only-pool.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the full-addressed constant pool should compile");

    // Relocated immediates remain zero in a relocatable object. The leading
    // `lis; addi` materializes the absolute address before the zero-displacement
    // load, proving that `-sdata2 0` reached codegen. The pool itself must
    // consequently live in `.rodata`.
    let expected = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,@constant@ha
        0x38, 0x63, 0x00, 0x00, // addi r3,r3,@constant@l
        0xc0, 0x03, 0x00, 0x00, // lfs f0,0(r3)
        0xec, 0x20, 0x00, 0x72, // fmuls f1,f0,f1
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
    assert!(object
        .windows(b".rodata\0".len())
        .any(|bytes| bytes == b".rodata\0"));
}
