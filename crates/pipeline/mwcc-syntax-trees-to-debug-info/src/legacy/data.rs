//! Legacy DWARF-1 records for functionless data translation units.

use super::{attribute, UNIT_END};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugRecord, FundamentalType, Tag,
};
use mwcc_syntax_trees::{AggregateDefinition, GlobalDeclaration, Pointee, TranslationUnit, Type};

const DATA_END: DebugEntryId = DebugEntryId(u32::MAX - 3);

enum PlanKind<'a> {
    Scalar,
    Array {
        type_id: DebugEntryId,
    },
    Aggregate {
        type_id: DebugEntryId,
        member_ids: Vec<DebugEntryId>,
        children_end: DebugEntryId,
        definition: &'a AggregateDefinition,
    },
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
) -> Compilation<Vec<DebugRecord>> {
    let mut next_id = first_id.0;
    let mut allocate = || {
        let id = DebugEntryId(next_id);
        next_id += 1;
        id
    };
    let mut plans = Vec::with_capacity(globals.len());
    for global in globals {
        let (start_id, global_id, kind) = if let Some(length) = global.array_length {
            if length == 0 {
                return Err(Diagnostic::error(format!(
                    "debug-info: zero-length array '{}' has no measured legacy subscript encoding",
                    global.name
                )));
            }
            fundamental_type(global.declared_type)?;
            let type_id = allocate();
            let global_id = allocate();
            (type_id, global_id, PlanKind::Array { type_id })
        } else if matches!(global.declared_type, Type::Struct { .. }) {
            let tag = unit
                .global_aggregate_tags
                .get(&global.name)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "debug-info: aggregate identity for global '{}' was not retained",
                        global.name
                    ))
                })?;
            let definition = unit.aggregate_definitions.get(tag).ok_or_else(|| {
                Diagnostic::error(format!(
                    "debug-info: aggregate definition '{tag}' was not retained"
                ))
            })?;
            if definition.is_union
                || definition.members.iter().any(|member| {
                    member.array_length.is_some()
                        || member.bit_field.is_some()
                        || matches!(
                            member.declared_type,
                            Type::Struct { .. } | Type::StructPointer { .. }
                        )
                })
            {
                return Err(Diagnostic::error(format!(
                    "debug-info: aggregate '{}' uses a member shape not implemented by legacy DWARF lowering yet (roadmap)",
                    definition.name
                )));
            }
            for member in &definition.members {
                member_type_attribute(member.declared_type)?;
            }
            let type_id = allocate();
            let member_ids = definition.members.iter().map(|_| allocate()).collect();
            let children_end = allocate();
            let global_id = allocate();
            (
                type_id,
                global_id,
                PlanKind::Aggregate {
                    type_id,
                    member_ids,
                    children_end,
                    definition,
                },
            )
        } else {
            global_type_attribute(global.declared_type, None)?;
            let global_id = allocate();
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
                global_type_attribute(plan.global.declared_type, None)?,
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
                            AttributeValue::Block2(subscript_data(
                                plan.global.array_length.unwrap(),
                                fundamental_type(plan.global.declared_type)?,
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
                type_id,
                member_ids,
                children_end,
                definition,
            } => {
                records.push(DebugRecord::Entry(DebugEntry {
                    id: *type_id,
                    tag: Tag::StructureType,
                    attributes: vec![
                        attribute(
                            AttributeName::Sibling,
                            AttributeValue::Reference(plan.global_id),
                        ),
                        attribute(
                            AttributeName::Name,
                            AttributeValue::String(definition.name.clone()),
                        ),
                        attribute(
                            AttributeName::ByteSize,
                            AttributeValue::Data4(definition.byte_size),
                        ),
                    ],
                }));
                for (member_index, member) in definition.members.iter().enumerate() {
                    let sibling = member_ids
                        .get(member_index + 1)
                        .copied()
                        .unwrap_or(*children_end);
                    records.push(DebugRecord::Entry(DebugEntry {
                        id: member_ids[member_index],
                        tag: Tag::Member,
                        attributes: vec![
                            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                            attribute(
                                AttributeName::Name,
                                AttributeValue::String(member.name.clone()),
                            ),
                            member_type_attribute(member.declared_type)?,
                            member_location(member.offset),
                        ],
                    }));
                }
                records.push(DebugRecord::Marker(*children_end));
                records.push(DebugRecord::Raw(vec![0, 0, 0, 4]));
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
        }
    }
    records.extend([
        DebugRecord::Marker(DATA_END),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ]);
    debug_assert_ne!(DATA_END, UNIT_END);
    Ok(records)
}

fn global_entry(
    global: &GlobalDeclaration,
    id: DebugEntryId,
    sibling: DebugEntryId,
    type_attribute: Attribute,
) -> DebugEntry {
    DebugEntry {
        id,
        tag: Tag::GlobalVariable,
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
    declared_type: Type,
    aggregate_id: Option<DebugEntryId>,
) -> Compilation<Attribute> {
    match declared_type {
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
        other => Ok(fundamental_attribute(fundamental_type(other)?)),
    }
}

fn member_type_attribute(declared_type: Type) -> Compilation<Attribute> {
    match declared_type {
        Type::Pointer(pointee) => Ok(modified_fundamental_type(pointee_type(pointee)?)),
        Type::Struct { .. } | Type::StructPointer { .. } => Err(Diagnostic::error(
            "debug-info: nested aggregate member types are not implemented yet (roadmap)",
        )),
        other => Ok(fundamental_attribute(fundamental_type(other)?)),
    }
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

fn subscript_data(length: u16, fundamental: FundamentalType) -> Vec<u8> {
    let mut bytes = vec![0, 0, 10];
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&u32::from(length - 1).to_be_bytes());
    bytes.extend_from_slice(&[8, 0, 0x55]);
    bytes.extend_from_slice(&(fundamental as u16).to_be_bytes());
    bytes
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
