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
    let identities = unit
        .function_return_enumeration_tags
        .values()
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
