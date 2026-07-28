use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

#[test]
fn keeps_equally_named_function_statics_in_distinct_storage() {
    let source = br#"
        void touch(int*);

        int first(void) {
            static int buffer;
            touch(&buffer);
            return buffer;
        }

        int second(void) {
            static int buffer;
            touch(&buffer);
            return buffer;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "duplicate-static-local-names.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("equally named statics in separate functions should compile");

    let buffers = symbols(&object)
        .into_iter()
        .filter(|(name, section, _, _)| name.starts_with("buffer$") && section == ".sbss")
        .collect::<Vec<_>>();
    assert_eq!(buffers.len(), 2);
    assert_ne!(buffers[0].2, buffers[1].2);
    assert!(!object.contains(&0x1f), "private link identities leaked into ELF");
}
