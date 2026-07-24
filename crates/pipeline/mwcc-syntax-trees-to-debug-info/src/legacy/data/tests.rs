use super::*;

#[test]
fn function_local_aggregates_extend_the_data_type_chain_before_functions() {
    let source = br#"
        typedef struct Color {
            unsigned char r, g, b, a;
        } Color;
        int enabled = 1;
        void paint(void) {
            Color foreground = { 1, 2, 3, 4 };
        }
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
    let local_key = unit
        .function_local_aggregate_tags
        .get(&("paint".to_owned(), "foreground".to_owned()))
        .expect("local aggregate identity")
        .clone();
    let lowered = records_with_local_aggregates_directly_followed_by_functions(
        &unit,
        &globals,
        &[local_key.clone()],
        DebugEntryId(1),
    )
    .expect("mixed data/type records");
    let aggregate_id = lowered.aggregate_ids[&local_key];

    let global = lowered
        .records
        .iter()
        .find_map(|record| match record {
            DebugRecord::Entry(entry) if entry.tag == Tag::GlobalVariable => Some(entry),
            _ => None,
        })
        .expect("global DIE");
    assert!(global.attributes.iter().any(|attribute| {
        attribute.name == AttributeName::Sibling
            && attribute.value == AttributeValue::Reference(aggregate_id)
    }));

    let aggregate = lowered
        .records
        .iter()
        .find_map(|record| match record {
            DebugRecord::Entry(entry) if entry.id == aggregate_id => Some(entry),
            _ => None,
        })
        .expect("local aggregate DIE");
    assert!(aggregate.attributes.iter().any(|attribute| {
        attribute.name == AttributeName::Sibling
            && attribute.value == AttributeValue::Reference(lowered.next_id)
    }));
}

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
        tags.iter()
            .filter(|tag| **tag == Tag::StructureType)
            .count(),
        1,
        "one source aggregate owns one translation-unit DIE"
    );
    assert_eq!(tags.iter().filter(|tag| **tag == Tag::ArrayType).count(), 3);
    assert_eq!(
        tags.iter()
            .filter(|tag| **tag == Tag::LocalVariable)
            .count(),
        2
    );
    assert_eq!(
        tags.iter()
            .filter(|tag| **tag == Tag::GlobalVariable)
            .count(),
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

#[test]
fn scalar_array_subscript_preserves_typedef_source_identity() {
    let source = br#"
        typedef unsigned long u32;
        u32 bitmap[4] = { 1, 2, 3, 4 };
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
    let subscript = lowered
        .records
        .iter()
        .find_map(|record| match record {
            DebugRecord::Entry(entry) if entry.tag == Tag::ArrayType => entry
                .attributes
                .iter()
                .find(|attribute| attribute.name == AttributeName::SubscriptData),
            DebugRecord::Entry(_) | DebugRecord::Marker(_) | DebugRecord::Raw(_) => None,
        })
        .expect("array subscript attribute");
    let AttributeValue::Block2(bytes) = &subscript.value else {
        panic!("scalar array uses an ordinary block")
    };

    assert_eq!(&bytes[bytes.len() - 2..], &[0, 0x0c]);
}

#[test]
fn aggregate_member_array_precedes_its_owner_and_is_referenced_by_the_member() {
    let source = br#"
        struct Packet {
            unsigned char code;
            unsigned char pad[3];
        };
        struct Packet packet = { 1, { 2, 3, 4 } };
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
    let entries = lowered
        .records
        .iter()
        .filter_map(|record| match record {
            DebugRecord::Entry(entry) => Some(entry),
            DebugRecord::Marker(_) | DebugRecord::Raw(_) => None,
        })
        .collect::<Vec<_>>();

    let structure_index = entries
        .iter()
        .position(|entry| entry.tag == Tag::StructureType)
        .expect("structure DIE");
    let array = entries
        .get(structure_index - 1)
        .filter(|entry| entry.tag == Tag::ArrayType)
        .expect("member-array DIE immediately before its owner");
    let subscript = array
        .attributes
        .iter()
        .find(|attribute| attribute.name == AttributeName::SubscriptData)
        .expect("array subscript attribute");
    assert_eq!(
        subscript.value,
        AttributeValue::Block2(vec![0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 2, 8, 0, 0x55, 0, 3,])
    );

    let pad = entries
        .iter()
        .find(|entry| {
            entry.tag == Tag::Member
                && entry.attributes.iter().any(|attribute| {
                    attribute.name == AttributeName::Name
                        && attribute.value == AttributeValue::String("pad".to_owned())
                })
        })
        .expect("pad member DIE");
    assert!(pad.attributes.iter().any(|attribute| {
        attribute.name == AttributeName::UserDefinedType
            && attribute.value == AttributeValue::Reference(array.id)
    }));
}

#[test]
fn local_only_aggregate_bridges_data_directly_to_following_functions() {
    let source = br#"
        typedef struct Color {
            unsigned char r;
            unsigned char g;
            unsigned char b;
            unsigned char a;
        } Color;
        int mode = 1;
        void paint(void) {
            Color foreground = { 1, 2, 3, 4 };
        }
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
    let key = unit.function_local_aggregate_tags
        [&("paint".to_owned(), "foreground".to_owned())]
        .clone();
    let lowered = records_with_local_aggregates_directly_followed_by_functions(
        &unit,
        &globals,
        &[key.clone()],
        DebugEntryId(1),
    )
    .expect("mixed data/local-type records");
    let entries = lowered
        .records
        .iter()
        .filter_map(|record| match record {
            DebugRecord::Entry(entry) => Some(entry),
            DebugRecord::Marker(_) | DebugRecord::Raw(_) => None,
        })
        .collect::<Vec<_>>();

    let global = entries
        .iter()
        .find(|entry| entry.tag == Tag::GlobalVariable)
        .expect("global DIE");
    let aggregate_id = lowered.aggregate_ids[&key];
    assert!(global.attributes.iter().any(|attribute| {
        attribute.name == AttributeName::Sibling
            && attribute.value == AttributeValue::Reference(aggregate_id)
    }));

    let aggregate = entries
        .iter()
        .find(|entry| entry.id == aggregate_id)
        .expect("local-only aggregate DIE");
    assert!(aggregate.attributes.iter().any(|attribute| {
        attribute.name == AttributeName::Sibling
            && attribute.value == AttributeValue::Reference(lowered.next_id)
    }));
    assert!(!lowered
        .records
        .iter()
        .any(|record| *record == DebugRecord::Marker(DATA_END)));
}
