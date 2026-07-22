use super::*;

#[test]
fn aggregate_arrays_share_types_and_preserve_variable_linkage() {
    let source = br#"
        typedef struct scroll_s {
            unsigned char x;
            unsigned char y;
        } Scroll;
        static Scroll first[] = { { 1, 2 }, { 3, 4 } };
        static Scroll second[] = { { 5, 6 }, { 7, 8 } };
        extern Scroll exported[] = { { 9, 10 }, { 11, 12 } };
    "#;
    let unit = mwcc_tokens_to_syntax_trees::parse_located_translation_unit(
        mwcc_source_to_tokens::tokenize_bytes_located(source).expect("tokens"),
        false,
        false,
        3,
        1,
    )
    .expect("translation unit");
    let globals = unit.globals.iter().collect::<Vec<_>>();
    let lowered = records(&unit, &globals, DebugEntryId(1), false).expect("data records");

    let tags = lowered
        .records
        .iter()
        .filter_map(|record| match record {
            DebugRecord::Entry(entry) => Some(entry.tag),
            DebugRecord::Marker(_) | DebugRecord::Raw(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tags.iter().filter(|tag| **tag == Tag::StructureType).count(),
        1,
        "one source aggregate owns one translation-unit DIE"
    );
    assert_eq!(
        tags.iter().filter(|tag| **tag == Tag::ArrayType).count(),
        3
    );
    assert_eq!(
        tags.iter().filter(|tag| **tag == Tag::LocalVariable).count(),
        2
    );
    assert_eq!(
        tags.iter().filter(|tag| **tag == Tag::GlobalVariable).count(),
        1
    );
}

#[test]
fn aggregate_array_subscript_uses_a_relocatable_user_type_reference() {
    let block = aggregate_subscript_data(2, DebugEntryId(7));
    assert_eq!(
        block.bytes,
        [0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 1, 8, 0, 0x72, 0, 0, 0, 0]
    );
    assert_eq!(block.relocations.len(), 1);
    assert_eq!(block.relocations[0].offset, 14);
    assert_eq!(
        block.relocations[0].address,
        Address::debug_entry(DebugEntryId(7))
    );
}
