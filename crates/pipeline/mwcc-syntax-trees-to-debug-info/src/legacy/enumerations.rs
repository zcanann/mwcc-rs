//! Legacy DWARF-1 enumeration declarations.
//!
//! Code generation sees an enum's integer storage type. This planner follows
//! the syntax tree's source-only enum graph and emits only definitions reached
//! by a function signature, preserving declaration order and keeping type DIE
//! scheduling separate from function DIE construction.

use mwcc_dwarf1::{
    Attribute, AttributeName, AttributeValue, DebugEntry, DebugEntryId, DebugRecord, Tag,
};
use mwcc_syntax_trees::{EnumerationDefinition, TranslationUnit};
use std::collections::{HashMap, HashSet};

pub(super) struct EnumerationPlan {
    pub records: Vec<DebugRecord>,
    pub ids: HashMap<String, DebugEntryId>,
}

pub(super) fn referenced<'a>(unit: &'a TranslationUnit) -> Vec<&'a EnumerationDefinition> {
    // The side map also retains prototypes encountered through headers. DWARF
    // for this object owns only definitions emitted by this translation unit;
    // pulling prototype-only enum identities into the closure creates unrelated
    // type DIEs (for example GX enums in a tiny runtime source).
    let identities = unit
        .functions
        .iter()
        .filter_map(|function| unit.function_return_enumeration_tags.get(&function.name))
        .collect::<HashSet<_>>();
    unit.enumeration_definitions
        .iter()
        .filter(|definition| identities.contains(&definition.name))
        .collect()
}

pub(super) fn records(
    definitions: &[&EnumerationDefinition],
    first_id: DebugEntryId,
    next_id: DebugEntryId,
) -> EnumerationPlan {
    let ids = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| {
            (
                definition.name.clone(),
                DebugEntryId(first_id.0 + index as u32),
            )
        })
        .collect::<HashMap<_, _>>();
    let records = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| {
            let sibling = definitions
                .get(index + 1)
                .and_then(|next| ids.get(&next.name).copied())
                .unwrap_or(next_id);
            let mut attributes = vec![attribute(
                AttributeName::Sibling,
                AttributeValue::Reference(sibling),
            )];
            if let Some(name) = &definition.source_name {
                attributes.push(attribute(
                    AttributeName::Name,
                    AttributeValue::String(name.clone()),
                ));
            }
            attributes.extend([
                attribute(
                    AttributeName::ByteSize,
                    AttributeValue::Data4(u32::from(definition.byte_size)),
                ),
                attribute(
                    AttributeName::ElementList,
                    AttributeValue::Block4(element_list(definition)),
                ),
            ]);
            DebugRecord::Entry(DebugEntry {
                id: ids[&definition.name],
                tag: Tag::EnumerationType,
                attributes,
            })
        })
        .collect();
    EnumerationPlan { records, ids }
}

fn element_list(definition: &EnumerationDefinition) -> Vec<u8> {
    let mut bytes = Vec::new();
    for enumerator in &definition.enumerators {
        bytes.extend_from_slice(&(enumerator.value as u32).to_be_bytes());
        bytes.extend_from_slice(enumerator.name.as_bytes());
        bytes.push(0);
    }
    bytes
}

fn attribute(name: AttributeName, value: AttributeValue) -> Attribute {
    Attribute { name, value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Enumerator;

    fn parse(source: &[u8]) -> TranslationUnit {
        mwcc_tokens_to_syntax_trees::parse_located_translation_unit(
            mwcc_source_to_tokens::tokenize_bytes_located(source).expect("tokens"),
            false,
            true,
            3,
            1,
        )
        .expect("translation unit")
    }

    #[test]
    fn prototype_only_return_enums_do_not_join_the_object_type_closure() {
        let unit = parse(
            br#"
                typedef enum HeaderResult { HeaderOk } HeaderResult;
                HeaderResult header_api(void);
                int local_definition(void) { return 1; }
            "#,
        );

        assert!(referenced(&unit).is_empty());
    }

    #[test]
    fn defined_function_return_enum_joins_the_object_type_closure() {
        let unit = parse(
            br#"
                typedef enum Result { Failed = -1, Ok = 0 } Result;
                Result acquire(void) { return Ok; }
            "#,
        );

        assert_eq!(
            referenced(&unit)
                .iter()
                .map(|definition| definition.source_name.as_deref())
                .collect::<Vec<_>>(),
            [Some("Result")]
        );
    }

    #[test]
    fn element_list_places_each_value_before_its_source_name() {
        let definition = EnumerationDefinition {
            name: "Error".into(),
            source_name: Some("Error".into()),
            byte_size: 4,
            enumerators: vec![
                Enumerator {
                    name: "Negative".into(),
                    value: -1,
                },
                Enumerator {
                    name: "Zero".into(),
                    value: 0,
                },
                Enumerator {
                    name: "Large".into(),
                    value: 0x701,
                },
            ],
        };

        assert_eq!(
            element_list(&definition),
            [
                &u32::MAX.to_be_bytes()[..],
                b"Negative\0",
                &0u32.to_be_bytes()[..],
                b"Zero\0",
                &0x701u32.to_be_bytes()[..],
                b"Large\0",
            ]
            .concat()
        );
    }
}
