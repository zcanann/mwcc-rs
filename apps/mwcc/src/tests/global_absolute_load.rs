use crate::{compile, SourceLanguage};

fn compile_getter(optimization: mwcc_versions::Optimization) -> Vec<u8> {
    let source = br#"
        static unsigned char enabled;

        unsigned char get_enabled(void) {
            return enabled;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.global_addressing = mwcc_versions::GlobalAddressing::Absolute;
    flags.optimization = optimization;
    compile(
        source,
        "global-absolute-load.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::WII_1_0,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the absolute byte-global getter should compile")
}

#[test]
fn optimized_absolute_global_load_folds_the_low_relocation_into_the_access() {
    let object = compile_getter(mwcc_versions::Optimization::O4);
    let expected = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,enabled@ha
        0x88, 0x63, 0x00, 0x00, // lbz r3,enabled@l(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn unoptimized_absolute_global_load_materializes_the_complete_address() {
    let object = compile_getter(mwcc_versions::Optimization::O0);
    let expected = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,enabled@ha
        0x38, 0x63, 0x00, 0x00, // addi r3,r3,enabled@l
        0x88, 0x63, 0x00, 0x00, // lbz r3,0(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
