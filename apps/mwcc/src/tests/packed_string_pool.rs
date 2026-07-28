use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

#[test]
fn coalesces_function_and_global_literals_into_one_tu_base() {
    let source = br#"
        static char* names[2] = {"global", "other"};

        char* first(void) {
            return "first";
        }

        char* second(void) {
            return "second";
        }

        char* indexed(unsigned index) {
            return names[index];
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
        "packed-string-pool.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("packed function and global literals should compile");

    let bases = symbols(&object)
        .into_iter()
        .filter(|(name, _, _, _)| name.starts_with("@stringBase"))
        .collect::<Vec<_>>();
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0].0, "@stringBase0");
    let packed = b"first\0second\0global\0other\0";
    assert!(object
        .windows(packed.len())
        .any(|bytes| bytes == packed));
}
