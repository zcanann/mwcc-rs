use crate::{compile, SourceLanguage};

#[test]
fn resolves_static_local_pointer_table_strings_into_the_function_pool() {
    let source = br#"
        char* choose(unsigned index) {
            static char* text[2] = {"first", "second"};
            return text[index];
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    flags.string_literals_packed = true;
    let object = compile(
        source,
        "static-local-string-table.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the static-local string pointer table should compile");

    for literal in [b"first\0".as_slice(), b"second\0".as_slice()] {
        assert!(object.windows(literal.len()).any(|bytes| bytes == literal));
    }
    assert!(!object
        .windows(b"@@str".len())
        .any(|bytes| bytes == b"@@str"));
}
