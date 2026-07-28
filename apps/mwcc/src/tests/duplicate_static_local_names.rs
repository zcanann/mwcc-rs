use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

#[test]
fn keeps_equally_named_function_statics_in_distinct_storage() {
    let source = br#"
        extern void touch(int*);

        namespace {
            int first() {
                static int buffer;
                touch(&buffer);
                return buffer;
            }

            int second() {
                static int buffer;
                touch(&buffer);
                return buffer;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "duplicate-static-local-names.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::Cxx),
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
