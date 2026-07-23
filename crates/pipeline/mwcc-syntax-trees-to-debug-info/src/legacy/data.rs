//! Data declarations and their reachable type graphs in legacy DWARF-1 units.

mod arrays;

#[cfg(test)]
mod tests;

use super::{attribute, UNIT_END};
use arrays::{aggregate_subscript_data, fundamental_subscript_data};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugRecord, FundamentalType, Tag,
};
use mwcc_syntax_trees::{
    AggregateDefinition, GlobalDeclaration, Pointee, SourceFundamentalType, TranslationUnit, Type,
};
use std::collections::HashMap;

const DATA_END: DebugEntryId = DebugEntryId(u32::MAX - 3);

pub(super) struct DataRecords {
    pub records: Vec<DebugRecord>,
    pub next_id: DebugEntryId,
    /// First emitted DIE for each retained aggregate identity. Function
    /// parameters can reference the same declaration graph that data globals
    /// caused to be materialized.
    pub aggregate_ids: HashMap<String, DebugEntryId>,
}

/// Semantic record order used by GC 4.x data-only units. The same plan drives
/// DIE construction and the later ELF-fragment partition, keeping source type
/// ownership out of the object-container layer.
pub(crate) enum FragmentedDataItem<'a> {
    Callable {
        function_type: &'a mwcc_syntax_trees::SourceFunctionType,
    },
    Aggregate {
        key: String,
        definition: &'a AggregateDefinition,
    },
    Global {
        global: &'a GlobalDeclaration,
        aggregate_key: String,
    },
}

pub(crate) fn fragmented_plan(unit: &TranslationUnit) -> Compilation<Vec<FragmentedDataItem<'_>>> {
    let globals = unit
        .globals
        .iter()
        .filter(|global| !global.is_extern && !global.is_static && !global.name.is_empty())
        .collect::<Vec<_>>();
    let mut ordered = globals
        .iter()
        .copied()
        .filter(|global| !is_tentative_zero(global))
        .collect::<Vec<_>>();
    ordered.extend(
        globals
            .iter()
            .rev()
            .copied()
            .filter(|global| is_tentative_zero(global)),
    );
    let root_keys = ordered
        .iter()
        .map(|global| {
            unit.global_aggregate_tags
                .get(&global.name)
                .cloned()
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "debug-info: aggregate identity for global '{}' was not retained",
                        global.name
                    ))
                })
        })
        .collect::<Compilation<Vec<_>>>()?;
    let mut emitted = Vec::<String>::new();
    let mut items = Vec::new();
    for (global, root_key) in ordered.into_iter().zip(root_keys.iter()) {
        collect_fragmented_types(
            unit,
            root_key,
            root_key,
            &root_keys,
            &mut emitted,
            &mut items,
        )?;
        items.push(FragmentedDataItem::Global {
            global,
            aggregate_key: root_key.clone(),
        });
    }
    Ok(items)
}

fn is_tentative_zero(global: &GlobalDeclaration) -> bool {
    global.initializer.is_none()
        && global.address_initializer.is_none()
        && global.data_bytes.is_none()
        && global.data_relocations.is_empty()
}

fn collect_fragmented_types<'a>(
    unit: &'a TranslationUnit,
    key: &str,
    root_key: &str,
    roots: &[String],
    emitted: &mut Vec<String>,
    output: &mut Vec<FragmentedDataItem<'a>>,
) -> Compilation<()> {
    if emitted.iter().any(|seen| seen == key) {
        return Ok(());
    }
    if key != root_key && roots.iter().any(|root| root == key) {
        return Ok(());
    }
    let definition = unit.aggregate_definitions.get(key).ok_or_else(|| {
        Diagnostic::error(format!(
            "debug-info: aggregate definition '{key}' was not retained"
        ))
    })?;
    for member in &definition.members {
        if matches!(member.declared_type, Type::Struct { .. }) {
            let member_key = member.aggregate_tag.as_deref().ok_or_else(|| {
                Diagnostic::error(format!(
                    "debug-info: aggregate identity for member '{}.{}' was not retained",
                    definition.name, member.name
                ))
            })?;
            collect_fragmented_types(unit, member_key, root_key, roots, emitted, output)?;
        }
    }
    let mut callable_types = Vec::new();
    for member in &definition.members {
        let Some(function_type) = member.function_type.as_ref() else {
            continue;
        };
        if callable_types
            .iter()
            .any(|seen: &&mwcc_syntax_trees::SourceFunctionType| *seen == function_type)
        {
            continue;
        }
        callable_types.push(function_type);
        output.push(FragmentedDataItem::Callable { function_type });
    }
    emitted.push(key.to_string());
    output.push(FragmentedDataItem::Aggregate {
        key: key.to_string(),
        definition,
    });
    Ok(())
}

pub(super) fn fragmented_records(
    unit: &TranslationUnit,
    first_id: DebugEntryId,
) -> Compilation<DataRecords> {
    struct PlannedItem<'a> {
        start_id: DebugEntryId,
        kind: PlannedKind<'a>,
    }
    enum PlannedKind<'a> {
        Callable(CallablePlan<'a>),
        Aggregate(AggregatePlan<'a>),
        Global {
            global: &'a GlobalDeclaration,
            aggregate_key: String,
            id: DebugEntryId,
        },
    }

    let plan = fragmented_plan(unit)?;
    let mut next_id = first_id.0;
    let mut aggregate_ids = HashMap::new();
    let mut callable_ids = Vec::new();
    let mut planned = Vec::with_capacity(plan.len());
    for item in plan {
        match item {
            FragmentedDataItem::Callable { function_type } => {
                validate_void_callable(function_type)?;
                let return_id = allocate(&mut next_id);
                let callable_id = allocate(&mut next_id);
                let parameter_id = allocate(&mut next_id);
                let children_end = allocate(&mut next_id);
                callable_ids.push((function_type, return_id));
                planned.push(PlannedItem {
                    start_id: return_id,
                    kind: PlannedKind::Callable(CallablePlan {
                        return_id,
                        callable_id,
                        parameter_id,
                        children_end,
                        function_type,
                    }),
                });
            }
            FragmentedDataItem::Aggregate { key, definition } => {
                let type_id = allocate(&mut next_id);
                aggregate_ids.insert(key, type_id);
                let member_ids = definition
                    .members
                    .iter()
                    .map(|_| allocate(&mut next_id))
                    .collect::<Vec<_>>();
                let children_end = allocate(&mut next_id);
                planned.push(PlannedItem {
                    start_id: type_id,
                    kind: PlannedKind::Aggregate(AggregatePlan {
                        type_id,
                        member_ids,
                        member_type_ids: Vec::new(),
                        member_callable_type_ids: Vec::new(),
                        member_array_type_ids: vec![None; definition.members.len()],
                        children_end,
                        definition,
                    }),
                });
            }
            FragmentedDataItem::Global {
                global,
                aggregate_key,
            } => {
                let id = allocate(&mut next_id);
                planned.push(PlannedItem {
                    start_id: id,
                    kind: PlannedKind::Global {
                        global,
                        aggregate_key,
                        id,
                    },
                });
            }
        }
    }
    for item in &mut planned {
        if let PlannedKind::Aggregate(aggregate) = &mut item.kind {
            aggregate.member_type_ids = aggregate
                .definition
                .members
                .iter()
                .map(|member| {
                    member
                        .aggregate_tag
                        .as_ref()
                        .and_then(|key| aggregate_ids.get(key).copied())
                })
                .collect();
            aggregate.member_callable_type_ids = aggregate
                .definition
                .members
                .iter()
                .map(|member| {
                    member.function_type.as_ref().and_then(|signature| {
                        callable_ids
                            .iter()
                            .find(|(candidate, _)| *candidate == signature)
                            .map(|(_, id)| *id)
                    })
                })
                .collect();
        }
    }

    let mut records = Vec::new();
    for (index, item) in planned.iter().enumerate() {
        let sibling = planned
            .get(index + 1)
            .map_or(DATA_END, |following| following.start_id);
        match &item.kind {
            PlannedKind::Callable(callable) => {
                records.extend(callable_records(callable, sibling)?);
            }
            PlannedKind::Aggregate(aggregate) => {
                let mut attributes = vec![attribute(
                    AttributeName::Sibling,
                    AttributeValue::Reference(sibling),
                )];
                if let Some(source_tag) = &aggregate.definition.source_tag {
                    attributes.push(attribute(
                        AttributeName::Name,
                        AttributeValue::String(source_tag.clone()),
                    ));
                }
                attributes.push(attribute(
                    AttributeName::ByteSize,
                    AttributeValue::Data4(aggregate.definition.byte_size),
                ));
                records.push(DebugRecord::Entry(DebugEntry {
                    id: aggregate.type_id,
                    tag: if aggregate.definition.is_union {
                        Tag::UnionType
                    } else {
                        Tag::StructureType
                    },
                    attributes,
                }));
                for (member_index, member) in aggregate.definition.members.iter().enumerate() {
                    let member_sibling = aggregate
                        .member_ids
                        .get(member_index + 1)
                        .copied()
                        .unwrap_or(aggregate.children_end);
                    let mut attributes = vec![
                        attribute(
                            AttributeName::Sibling,
                            AttributeValue::Reference(member_sibling),
                        ),
                        attribute(
                            AttributeName::Name,
                            AttributeValue::String(member.name.clone()),
                        ),
                        aggregate.member_callable_type_ids[member_index].map_or_else(
                            || {
                                member_type_attribute(
                                    member.declared_type,
                                    aggregate.member_type_ids[member_index],
                                    member.source_fundamental,
                                )
                            },
                            |id| Ok(modified_user_defined_type_with_modifier(id, 1)),
                        )?,
                    ];
                    if member.function_type.is_some() {
                        attributes.push(attribute(
                            AttributeName::MwMemberFlags,
                            AttributeValue::String(String::new()),
                        ));
                    }
                    attributes.push(member_location(member.offset));
                    records.push(DebugRecord::Entry(DebugEntry {
                        id: aggregate.member_ids[member_index],
                        tag: Tag::Member,
                        attributes,
                    }));
                }
                records.push(DebugRecord::Marker(aggregate.children_end));
                records.push(DebugRecord::Raw(vec![0, 0, 0, 4]));
            }
            PlannedKind::Global {
                global,
                aggregate_key,
                id,
            } => records.push(DebugRecord::Entry(global_entry(
                global,
                *id,
                sibling,
                global_type_attribute(global, aggregate_ids.get(aggregate_key).copied())?,
            ))),
        }
    }
    records.push(DebugRecord::Marker(DATA_END));
    records.extend([
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ]);
    Ok(DataRecords {
        records,
        next_id: DebugEntryId(next_id),
        aggregate_ids,
    })
}

enum PlanKind<'a> {
    Scalar,
    Array {
        type_id: DebugEntryId,
    },
    Aggregate {
        root_type_id: DebugEntryId,
        array_type_id: Option<DebugEntryId>,
        types: Vec<AggregatePlan<'a>>,
    },
}

struct AggregatePlan<'a> {
    type_id: DebugEntryId,
    member_ids: Vec<DebugEntryId>,
    member_type_ids: Vec<Option<DebugEntryId>>,
    member_callable_type_ids: Vec<Option<DebugEntryId>>,
    /// Array type emitted immediately before this aggregate, by member index.
    /// Members reference the array DIE rather than their scalar element type.
    member_array_type_ids: Vec<Option<DebugEntryId>>,
    children_end: DebugEntryId,
    definition: &'a AggregateDefinition,
}

struct CallablePlan<'a> {
    return_id: DebugEntryId,
    callable_id: DebugEntryId,
    parameter_id: DebugEntryId,
    children_end: DebugEntryId,
    function_type: &'a mwcc_syntax_trees::SourceFunctionType,
}

impl AggregatePlan<'_> {
    fn start_id(&self) -> DebugEntryId {
        self.member_array_type_ids
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(self.type_id)
    }
}

struct GlobalPlan<'a> {
    global: &'a GlobalDeclaration,
    start_id: DebugEntryId,
    global_id: DebugEntryId,
    kind: PlanKind<'a>,
}

pub(super) fn records<'a>(
    unit: &'a TranslationUnit,
    globals: &[&'a GlobalDeclaration],
    first_id: DebugEntryId,
    has_following_functions: bool,
) -> Compilation<DataRecords> {
    let mut next_id = first_id.0;
    let mut plans = Vec::with_capacity(globals.len());
    let mut aggregate_ids = HashMap::new();
    for global in globals {
        let (start_id, global_id, kind) = if matches!(global.declared_type, Type::Struct { .. }) {
            let tag = unit
                .global_aggregate_tags
                .get(&global.name)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "debug-info: aggregate identity for global '{}' was not retained",
                        global.name
                    ))
                })?;
            let mut types = Vec::new();
            // Aggregate identities are translation-unit scoped. Emit each
            // reachable type graph at its first data declaration, then let
            // later globals reference the already-emitted DIE.
            let root_type_id =
                plan_aggregate(unit, tag, &mut next_id, &mut aggregate_ids, &mut types)?;
            let array_type_id = global
                .array_length
                .map(|length| {
                    if length == 0 {
                        return Err(Diagnostic::error(format!(
                            "debug-info: zero-length array '{}' has no measured legacy subscript encoding",
                            global.name
                        )));
                    }
                    Ok(allocate(&mut next_id))
                })
                .transpose()?;
            let global_id = allocate(&mut next_id);
            let start_id = types
                .first()
                .map(AggregatePlan::start_id)
                .or(array_type_id)
                .unwrap_or(global_id);
            (
                start_id,
                global_id,
                PlanKind::Aggregate {
                    root_type_id,
                    array_type_id,
                    types,
                },
            )
        } else if let Some(length) = global.array_length {
            if length == 0 {
                return Err(Diagnostic::error(format!(
                    "debug-info: zero-length array '{}' has no measured legacy subscript encoding",
                    global.name
                )));
            }
            global_fundamental_type(global)?;
            let type_id = allocate(&mut next_id);
            let global_id = allocate(&mut next_id);
            (type_id, global_id, PlanKind::Array { type_id })
        } else {
            global_type_attribute(global, None)?;
            let global_id = allocate(&mut next_id);
            (global_id, global_id, PlanKind::Scalar)
        };
        plans.push(GlobalPlan {
            global,
            start_id,
            global_id,
            kind,
        });
    }

    let mut records = Vec::new();
    for (index, plan) in plans.iter().enumerate() {
        let next = plans
            .get(index + 1)
            .map_or(DATA_END, |following| following.start_id);
        match &plan.kind {
            PlanKind::Scalar => records.push(DebugRecord::Entry(global_entry(
                plan.global,
                plan.global_id,
                next,
                global_type_attribute(plan.global, None)?,
            ))),
            PlanKind::Array { type_id } => {
                records.push(DebugRecord::Entry(DebugEntry {
                    id: *type_id,
                    tag: Tag::ArrayType,
                    attributes: vec![
                        attribute(
                            AttributeName::Sibling,
                            AttributeValue::Reference(plan.global_id),
                        ),
                        attribute(
                            AttributeName::SubscriptData,
                            AttributeValue::Block2(fundamental_subscript_data(
                                plan.global.array_length.unwrap(),
                                global_fundamental_type(plan.global)?,
                            )),
                        ),
                    ],
                }));
                records.push(DebugRecord::Entry(global_entry(
                    plan.global,
                    plan.global_id,
                    next,
                    attribute(
                        AttributeName::UserDefinedType,
                        AttributeValue::Reference(*type_id),
                    ),
                )));
            }
            PlanKind::Aggregate {
                root_type_id,
                array_type_id,
                types,
            } => {
                for (type_index, aggregate) in types.iter().enumerate() {
                    let sibling = types.get(type_index + 1).map_or(
                        array_type_id.unwrap_or(plan.global_id),
                        AggregatePlan::start_id,
                    );
                    let array_ids = aggregate
                        .member_array_type_ids
                        .iter()
                        .flatten()
                        .copied()
                        .collect::<Vec<_>>();
                    for (array_index, (member_index, array_id)) in aggregate
                        .member_array_type_ids
                        .iter()
                        .enumerate()
                        .filter_map(|(member_index, id)| id.map(|id| (member_index, id)))
                        .enumerate()
                    {
                        let member = &aggregate.definition.members[member_index];
                        let array_length = u16::try_from(member.array_length.unwrap()).map_err(
                            |_| {
                                Diagnostic::error(format!(
                                    "debug-info: aggregate '{}.{}' has a member array too large for legacy DWARF",
                                    aggregate.definition.name, member.name
                                ))
                            },
                        )?;
                        records.push(DebugRecord::Entry(DebugEntry {
                            id: array_id,
                            tag: Tag::ArrayType,
                            attributes: vec![
                                attribute(
                                    AttributeName::Sibling,
                                    AttributeValue::Reference(
                                        array_ids
                                            .get(array_index + 1)
                                            .copied()
                                            .unwrap_or(aggregate.type_id),
                                    ),
                                ),
                                attribute(
                                    AttributeName::SubscriptData,
                                    AttributeValue::Block2(fundamental_subscript_data(
                                        array_length,
                                        aggregate_member_fundamental_type(member)?,
                                    )),
                                ),
                            ],
                        }));
                    }
                    let mut attributes = vec![attribute(
                        AttributeName::Sibling,
                        AttributeValue::Reference(sibling),
                    )];
                    if let Some(source_tag) = &aggregate.definition.source_tag {
                        attributes.push(attribute(
                            AttributeName::Name,
                            AttributeValue::String(source_tag.clone()),
                        ));
                    }
                    attributes.push(attribute(
                        AttributeName::ByteSize,
                        AttributeValue::Data4(aggregate.definition.byte_size),
                    ));
                    records.push(DebugRecord::Entry(DebugEntry {
                        id: aggregate.type_id,
                        tag: if aggregate.definition.is_union {
                            Tag::UnionType
                        } else {
                            Tag::StructureType
                        },
                        attributes,
                    }));
                    for (member_index, member) in aggregate.definition.members.iter().enumerate() {
                        let member_sibling = aggregate
                            .member_ids
                            .get(member_index + 1)
                            .copied()
                            .unwrap_or(aggregate.children_end);
                        records.push(DebugRecord::Entry(DebugEntry {
                            id: aggregate.member_ids[member_index],
                            tag: Tag::Member,
                            attributes: vec![
                                attribute(
                                    AttributeName::Sibling,
                                    AttributeValue::Reference(member_sibling),
                                ),
                                attribute(
                                    AttributeName::Name,
                                    AttributeValue::String(member.name.clone()),
                                ),
                                aggregate.member_array_type_ids[member_index].map_or_else(
                                    || {
                                        member_type_attribute(
                                            member.declared_type,
                                            aggregate.member_type_ids[member_index],
                                            member.source_fundamental,
                                        )
                                    },
                                    |id| {
                                        Ok(attribute(
                                            AttributeName::UserDefinedType,
                                            AttributeValue::Reference(id),
                                        ))
                                    },
                                )?,
                                member_location(member.offset),
                            ],
                        }));
                    }
                    records.push(DebugRecord::Marker(aggregate.children_end));
                    records.push(DebugRecord::Raw(vec![0, 0, 0, 4]));
                }
                if let Some(type_id) = array_type_id {
                    records.push(DebugRecord::Entry(DebugEntry {
                        id: *type_id,
                        tag: Tag::ArrayType,
                        attributes: vec![
                            attribute(
                                AttributeName::Sibling,
                                AttributeValue::Reference(plan.global_id),
                            ),
                            attribute(
                                AttributeName::SubscriptData,
                                AttributeValue::RelocatableBlock2(aggregate_subscript_data(
                                    plan.global.array_length.unwrap(),
                                    *root_type_id,
                                )),
                            ),
                        ],
                    }));
                }
                records.push(DebugRecord::Entry(global_entry(
                    plan.global,
                    plan.global_id,
                    next,
                    attribute(
                        AttributeName::UserDefinedType,
                        AttributeValue::Reference(array_type_id.unwrap_or(*root_type_id)),
                    ),
                )));
            }
        }
    }
    records.push(DebugRecord::Marker(DATA_END));
    if !has_following_functions {
        records.extend([
            DebugRecord::Raw(vec![0, 0, 0, 4]),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
        ]);
    }
    debug_assert_ne!(DATA_END, UNIT_END);
    Ok(DataRecords {
        records,
        next_id: DebugEntryId(next_id),
        aggregate_ids,
    })
}

fn allocate(next_id: &mut u32) -> DebugEntryId {
    let id = DebugEntryId(*next_id);
    *next_id += 1;
    id
}

fn validate_void_callable(
    function_type: &mwcc_syntax_trees::SourceFunctionType,
) -> Compilation<()> {
    let result = &function_type.return_type;
    if function_type.variadic
        || !function_type.parameters.is_empty()
        || result.declared_type != Type::Void
        || result.source_fundamental != Some(SourceFundamentalType::Void)
        || result.pointer_depth != 0
        || result.is_reference
        || result.function_type.is_some()
    {
        return Err(Diagnostic::error(
            "debug-info: this function-pointer member signature is not implemented yet (roadmap)",
        ));
    }
    Ok(())
}

fn callable_records(
    callable: &CallablePlan<'_>,
    sibling: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    validate_void_callable(callable.function_type)?;
    Ok(vec![
        DebugRecord::Entry(DebugEntry {
            id: callable.return_id,
            tag: Tag::ModifiedType,
            attributes: vec![
                attribute(
                    AttributeName::Sibling,
                    AttributeValue::Reference(callable.callable_id),
                ),
                fundamental_attribute(FundamentalType::Void),
            ],
        }),
        DebugRecord::Entry(DebugEntry {
            id: callable.callable_id,
            tag: Tag::ModifiedType,
            attributes: vec![
                attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                modified_user_defined_type_with_modifier(sibling, 2),
            ],
        }),
        DebugRecord::Entry(DebugEntry {
            id: callable.parameter_id,
            tag: Tag::FormalParameter,
            attributes: vec![
                attribute(
                    AttributeName::Sibling,
                    AttributeValue::Reference(callable.children_end),
                ),
                modified_user_defined_type_with_modifier(sibling, 2),
            ],
        }),
        DebugRecord::Marker(callable.children_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ])
}

/// Build the reachable aggregate type graph in MWCC's measured postorder:
/// dependencies appear before the aggregate that references them, and a shared
/// dependency is emitted only once for each global's type graph.
fn plan_aggregate<'a>(
    unit: &'a TranslationUnit,
    tag: &str,
    next_id: &mut u32,
    type_ids: &mut HashMap<String, DebugEntryId>,
    plans: &mut Vec<AggregatePlan<'a>>,
) -> Compilation<DebugEntryId> {
    if let Some(id) = type_ids.get(tag) {
        return Ok(*id);
    }
    let definition = unit.aggregate_definitions.get(tag).ok_or_else(|| {
        Diagnostic::error(format!(
            "debug-info: aggregate definition '{tag}' was not retained"
        ))
    })?;
    // Register the type before descending so self-referential pointers close
    // the cycle without recursively duplicating the type.
    let type_id = allocate(next_id);
    type_ids.insert(tag.to_owned(), type_id);
    let mut member_type_ids = Vec::with_capacity(definition.members.len());
    let mut member_array_type_ids = Vec::with_capacity(definition.members.len());
    for member in &definition.members {
        if member.bit_field.is_some() {
            return Err(Diagnostic::error(format!(
                "debug-info: aggregate '{}' uses a bit-field member not implemented by legacy DWARF lowering yet (roadmap)",
                definition.name
            )));
        }
        if member.array_length == Some(0) {
            return Err(Diagnostic::error(format!(
                "debug-info: aggregate '{}.{}' has a zero-length member array",
                definition.name, member.name
            )));
        }
        let referenced_type = match member.declared_type {
            Type::Struct { .. } | Type::StructPointer { .. } => {
                let member_tag = member.aggregate_tag.as_deref().ok_or_else(|| {
                    Diagnostic::error(format!(
                        "debug-info: aggregate identity for member '{}.{}' was not retained",
                        definition.name, member.name
                    ))
                })?;
                Some(plan_aggregate(unit, member_tag, next_id, type_ids, plans)?)
            }
            _ => {
                member_type_attribute(member.declared_type, None, member.source_fundamental)?;
                None
            }
        };
        member_type_ids.push(referenced_type);
        member_array_type_ids.push(member.array_length.map(|_| allocate(next_id)));
    }
    let member_ids = definition
        .members
        .iter()
        .map(|_| allocate(next_id))
        .collect();
    let children_end = allocate(next_id);
    plans.push(AggregatePlan {
        type_id,
        member_ids,
        member_type_ids,
        member_callable_type_ids: vec![None; definition.members.len()],
        member_array_type_ids,
        children_end,
        definition,
    });
    Ok(type_id)
}

fn aggregate_member_fundamental_type(
    member: &mwcc_syntax_trees::AggregateMember,
) -> Compilation<FundamentalType> {
    match member.source_fundamental {
        Some(source) => source_fundamental_type(source),
        None => fundamental_type(member.declared_type),
    }
}

fn global_entry(
    global: &GlobalDeclaration,
    id: DebugEntryId,
    sibling: DebugEntryId,
    type_attribute: Attribute,
) -> DebugEntry {
    DebugEntry {
        id,
        tag: if global.is_static {
            Tag::LocalVariable
        } else {
            Tag::GlobalVariable
        },
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(
                AttributeName::Name,
                AttributeValue::String(global.name.clone()),
            ),
            type_attribute,
            attribute(
                AttributeName::Location,
                AttributeValue::RelocatableBlock2(Block {
                    bytes: vec![0x03, 0, 0, 0, 0],
                    relocations: vec![BlockRelocation {
                        offset: 1,
                        address: Address::external(&global.name),
                    }],
                }),
            ),
        ],
    }
}

fn global_type_attribute(
    global: &GlobalDeclaration,
    aggregate_id: Option<DebugEntryId>,
) -> Compilation<Attribute> {
    match global.declared_type {
        Type::Struct { .. } => aggregate_id
            .map(|id| {
                attribute(
                    AttributeName::UserDefinedType,
                    AttributeValue::Reference(id),
                )
            })
            .ok_or_else(|| Diagnostic::error("debug-info: a struct type needs an aggregate DIE")),
        Type::Pointer(pointee) => Ok(modified_fundamental_type(pointee_type(pointee)?)),
        Type::StructPointer { .. } => Err(Diagnostic::error(
            "debug-info: a struct pointer needs retained aggregate identity (roadmap)",
        )),
        _ => Ok(fundamental_attribute(global_fundamental_type(global)?)),
    }
}

fn global_fundamental_type(global: &GlobalDeclaration) -> Compilation<FundamentalType> {
    match (global.declared_type, global.source_fundamental) {
        (Type::Pointer(_) | Type::StructPointer { .. } | Type::Struct { .. }, _) => {
            fundamental_type(global.declared_type)
        }
        (_, Some(source)) => source_fundamental_type(source),
        (_, None) => fundamental_type(global.declared_type),
    }
}

pub(super) fn member_type_attribute(
    declared_type: Type,
    aggregate_id: Option<DebugEntryId>,
    source_fundamental: Option<SourceFundamentalType>,
) -> Compilation<Attribute> {
    match declared_type {
        Type::Pointer(_) if source_fundamental == Some(SourceFundamentalType::Void) => {
            Ok(fundamental_attribute(FundamentalType::Pointer))
        }
        Type::Pointer(pointee) => Ok(modified_fundamental_type(match source_fundamental {
            Some(source) => source_fundamental_type(source)?,
            None => pointee_type(pointee)?,
        })),
        Type::Struct { .. } => aggregate_id
            .map(|id| {
                attribute(
                    AttributeName::UserDefinedType,
                    AttributeValue::Reference(id),
                )
            })
            .ok_or_else(|| Diagnostic::error("debug-info: a struct member needs an aggregate DIE")),
        Type::StructPointer { .. } => {
            aggregate_id.map(modified_user_defined_type).ok_or_else(|| {
                Diagnostic::error("debug-info: a struct pointer member needs an aggregate DIE")
            })
        }
        other => Ok(fundamental_attribute(match source_fundamental {
            Some(source) => source_fundamental_type(source)?,
            None => fundamental_type(other)?,
        })),
    }
}

fn source_fundamental_type(source: SourceFundamentalType) -> Compilation<FundamentalType> {
    Ok(match source {
        SourceFundamentalType::Boolean => FundamentalType::Boolean,
        SourceFundamentalType::PlainChar => FundamentalType::Char,
        SourceFundamentalType::SignedChar => FundamentalType::SignedChar,
        SourceFundamentalType::UnsignedChar => FundamentalType::UnsignedChar,
        SourceFundamentalType::SignedShort => FundamentalType::SignedShort,
        SourceFundamentalType::UnsignedShort => FundamentalType::UnsignedShort,
        SourceFundamentalType::SignedInteger => FundamentalType::SignedInteger,
        SourceFundamentalType::UnsignedInteger => FundamentalType::UnsignedInteger,
        SourceFundamentalType::SignedLong => FundamentalType::SignedLong,
        SourceFundamentalType::UnsignedLong => FundamentalType::UnsignedLong,
        SourceFundamentalType::Float => FundamentalType::Float,
        SourceFundamentalType::Double => FundamentalType::Double,
        SourceFundamentalType::Void => FundamentalType::Void,
        SourceFundamentalType::SignedLongLong => FundamentalType::SignedLongLong,
        SourceFundamentalType::UnsignedLongLong => {
            return Err(Diagnostic::error(
                "debug-info: unsigned long long has no measured legacy fundamental encoding yet",
            ))
        }
    })
}

fn modified_user_defined_type(id: DebugEntryId) -> Attribute {
    modified_user_defined_type_with_modifier(id, 1)
}

fn modified_user_defined_type_with_modifier(id: DebugEntryId, modifier: u8) -> Attribute {
    attribute(
        AttributeName::ModifiedUserDefinedType,
        AttributeValue::RelocatableBlock2(Block {
            bytes: vec![modifier, 0, 0, 0, 0],
            relocations: vec![BlockRelocation {
                offset: 1,
                address: Address::debug_entry(id),
            }],
        }),
    )
}

fn fundamental_attribute(fundamental: FundamentalType) -> Attribute {
    attribute(
        AttributeName::FundamentalType,
        AttributeValue::Data2(fundamental as u16),
    )
}

fn modified_fundamental_type(fundamental: FundamentalType) -> Attribute {
    let [high, low] = (fundamental as u16).to_be_bytes();
    attribute(
        AttributeName::ModifiedFundamentalType,
        AttributeValue::Block2(vec![1, high, low]),
    )
}

fn member_location(offset: u32) -> Attribute {
    let mut bytes = vec![4];
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.push(7);
    attribute(AttributeName::Location, AttributeValue::Block2(bytes))
}

fn pointee_type(pointee: Pointee) -> Compilation<FundamentalType> {
    fundamental_type(pointee.element())
}

fn fundamental_type(declared_type: Type) -> Compilation<FundamentalType> {
    let fundamental = match declared_type {
        Type::Int => FundamentalType::SignedInteger,
        Type::UnsignedInt => FundamentalType::UnsignedInteger,
        Type::Char => FundamentalType::SignedChar,
        Type::UnsignedChar => FundamentalType::UnsignedChar,
        Type::Short => FundamentalType::SignedShort,
        Type::UnsignedShort => FundamentalType::UnsignedShort,
        Type::Float => FundamentalType::Float,
        Type::Double => FundamentalType::Double,
        Type::LongLong => FundamentalType::SignedLongLong,
        Type::Void => FundamentalType::Void,
        Type::UnsignedLongLong
        | Type::Pointer(_)
        | Type::StructPointer { .. }
        | Type::Struct { .. } => {
            return Err(Diagnostic::error(format!(
                "debug-info: fundamental mapping for {declared_type:?} is not implemented yet (roadmap)"
            )))
        }
    };
    Ok(fundamental)
}
