use crate::{compile, SourceLanguage};

#[test]
fn eliminates_unused_constant_initialized_automatic_arrays() {
    let source = br#"
        void dead_arrays(void) {
            const char first[12] = {};
            const char second[12] = {};
            const char large[40] = {};
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "dead-automatic-arrays.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("unused constant automatic arrays should optimize away");

    assert!(object
        .windows(4)
        .any(|bytes| bytes == [0x4e, 0x80, 0x00, 0x20]));
}
