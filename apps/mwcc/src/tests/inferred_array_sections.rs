use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

#[test]
fn legacy_inferred_array_section_quirk_applies_only_to_external_linkage() {
    let source = br#"
        int external_unsized[] = { 1 };
        static int static_unsized[] = { 2 };
        int external_sized[1] = { 3 };
        static int static_sized[1] = { 4 };
        int* targets[] = { external_unsized };
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "inferred-array-sections.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the inferred arrays should compile");
    let symbols = symbols(&object);
    let section = |name: &str| {
        symbols
            .iter()
            .find(|(symbol, _, _, _)| symbol == name)
            .map(|(_, section, _, _)| section.as_str())
            .unwrap()
    };

    // GC/1.2.5n build 163 bypasses the small-data threshold only for the
    // externally linked inferred-length form. Static inferred arrays follow
    // the same size-based routing as explicitly sized arrays.
    assert_eq!(section("external_unsized"), ".data");
    assert_eq!(section("static_unsized"), ".sdata");
    assert_eq!(section("external_sized"), ".sdata");
    assert_eq!(section("static_sized"), ".sdata");
    let symbol_position = |name: &str| {
        symbols
            .iter()
            .position(|(symbol, _, _, _)| symbol == name)
            .unwrap()
    };
    assert!(symbol_position("...data.0") < symbol_position("static_unsized"));
}
