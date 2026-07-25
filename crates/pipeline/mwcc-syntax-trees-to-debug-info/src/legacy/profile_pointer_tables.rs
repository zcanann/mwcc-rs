//! Debug plan for aggregate-pointer registries installed by module entry points.
//!
//! The legacy compiler keeps the registry's reachable aggregate graph, an
//! externally-owned pointer-to-pointer, and vendor references from each
//! lifecycle function in one uninterrupted DIE stream.  Keep recognition and
//! ordering together so this does not leak source-specific policy into the
//! generic data or function encoders.

#[cfg(test)]
mod tests;

use super::{attribute, data, functions};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugRecord, LineRecord, Tag,
};
use mwcc_machine_code::MachineFunction;
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    AggregateDefinition, Expression, Function, FunctionSource, GlobalDeclaration, Statement,
    TranslationUnit, Type,
};
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

struct Plan<'a> {
    table: &'a GlobalDeclaration,
    destination: &'a GlobalDeclaration,
    root: &'a AggregateDefinition,
    method: &'a AggregateDefinition,
}

pub(super) fn matches(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    emitted_globals: &[&GlobalDeclaration],
    build: CompilerBuild,
) -> bool {
    build.version == (2, 4, 2)
        && build.build == 81
        && unit.functions.len() == 2
        && machine_functions.len() == 2
        && unit
            .functions
            .iter()
            .zip(machine_functions)
            .all(|(source, machine)| source.name == machine.name)
        && recognize(unit, emitted_globals).is_some()
}

fn recognize<'a>(
    unit: &'a TranslationUnit,
    emitted_globals: &[&'a GlobalDeclaration],
) -> Option<Plan<'a>> {
    let [table] = emitted_globals else {
        return None;
    };
    let length = table.array_length?;
    if length == 0 || !matches!(table.declared_type, Type::StructPointer { .. }) {
        return None;
    }
    let root_key = unit.global_aggregate_tags.get(&table.name)?;
    let root = unit.aggregate_definitions.get(root_key)?;
    let method_key = root
        .members
        .iter()
        .filter_map(|member| {
            matches!(member.declared_type, Type::StructPointer { .. })
                .then_some(member.aggregate_tag.as_deref())
                .flatten()
        })
        .find(|key| {
            unit.aggregate_definitions
                .get(*key)
                .is_some_and(|definition| {
                    !definition.members.is_empty()
                        && definition
                            .members
                            .iter()
                            .all(|member| member.function_type.is_some())
                })
        })?;
    let method = unit.aggregate_definitions.get(method_key)?;
    let signature = method.members.first()?.function_type.as_ref()?;
    if method
        .members
        .iter()
        .any(|member| member.function_type.as_ref() != Some(signature))
        || signature.variadic
        || signature.parameters.len() != 1
        || signature.return_type.pointer_depth != 0
        || signature.return_type.is_reference
        || signature.return_type.function_type.is_some()
        || signature.parameters[0].pointer_depth != 1
        || signature.parameters[0].is_reference
        || signature.parameters[0].function_type.is_some()
    {
        return None;
    }

    let [install, clear] = unit.functions.as_slice() else {
        return None;
    };
    if !plain_lifecycle_function(install) || !plain_lifecycle_function(clear) {
        return None;
    }
    let (destination_name, installed_name) = store_variables(&install.statements[0])?;
    if installed_name != table.name {
        return None;
    }
    if !matches!(
        &clear.statements[0],
        Statement::Store {
            target: Expression::Variable(target),
            value: Expression::IntegerLiteral(0),
        } if target == destination_name
    ) {
        return None;
    }
    let destination = unit.globals.iter().find(|global| {
        global.name == destination_name
            && global.is_extern
            && global.array_length.is_none()
            && global.declared_type == Type::Pointer(mwcc_syntax_trees::Pointee::Pointer)
    })?;

    Some(Plan {
        table,
        destination,
        root,
        method,
    })
}

fn plain_lifecycle_function(function: &Function) -> bool {
    function.return_type == Type::Void
        && !function.is_static
        && function.parameters.is_empty()
        && function.locals.is_empty()
        && function.statements.len() == 1
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
}

fn store_variables(statement: &Statement) -> Option<(&str, &str)> {
    match statement {
        Statement::Store {
            target: Expression::Variable(target),
            value: Expression::Variable(value),
        } => Some((target, value)),
        _ => None,
    }
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    if functions.len() != 2
        || machine_functions.len() != 2
        || layout.offsets.len() != 2
        || layout.sizes.len() != 2
    {
        return Err(invalid_plan());
    }
    let mut records = Vec::with_capacity(4);
    for (index, ((function, source), machine)) in
        functions.iter().zip(machine_functions).enumerate()
    {
        if function.statements.len() != 1
            || source.statement_lines.len() != 1
            || layout.sizes[index] < 4
            || machine.text_deferred
        {
            return Err(invalid_plan());
        }
        records.extend([
            LineRecord {
                line: source.statement_lines[0],
                column: u16::MAX,
                address_delta: layout.offsets[index],
            },
            LineRecord {
                line: source.body_end_line,
                column: u16::MAX,
                address_delta: layout.offsets[index] + layout.sizes[index] - 4,
            },
        ]);
    }
    Ok(records)
}

pub(super) fn records(
    unit: &TranslationUnit,
    source_functions: &[&Function],
    layout: &FunctionLayout,
    emitted_globals: &[&GlobalDeclaration],
    first_id: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    let plan = recognize(unit, emitted_globals).ok_or_else(invalid_plan)?;
    let signature = plan.method.members[0]
        .function_type
        .as_ref()
        .ok_or_else(invalid_plan)?;
    let mut next = first_id.0;
    let callable_return = allocate(&mut next);
    let callable_parameter = allocate(&mut next);
    let callable_end = allocate(&mut next);
    let method_pointer = allocate(&mut next);
    let method_pointer_parameter = allocate(&mut next);
    let method_pointer_end = allocate(&mut next);
    let method_type = allocate(&mut next);
    let method_members = plan
        .method
        .members
        .iter()
        .map(|_| allocate(&mut next))
        .collect::<Vec<_>>();
    let method_end = allocate(&mut next);
    let root_pointer = allocate(&mut next);
    let root_pointer_parameter = allocate(&mut next);
    let root_pointer_end = allocate(&mut next);
    let root_type = allocate(&mut next);
    let root_members = plan
        .root
        .members
        .iter()
        .map(|_| allocate(&mut next))
        .collect::<Vec<_>>();
    let root_end = allocate(&mut next);
    let table_array = allocate(&mut next);
    let table_global = allocate(&mut next);
    let destination_global = allocate(&mut next);

    let mut records = vec![
        entry(
            callable_return,
            Tag::ModifiedType,
            vec![
                sibling(method_pointer),
                data::member_type_attribute(
                    signature.return_type.declared_type,
                    None,
                    signature.return_type.source_fundamental,
                )?,
            ],
        ),
        entry(
            callable_parameter,
            Tag::FormalParameter,
            vec![
                sibling(callable_end),
                data::member_type_attribute(
                    signature.parameters[0].declared_type,
                    None,
                    signature.parameters[0].source_fundamental,
                )?,
            ],
        ),
        DebugRecord::Marker(callable_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        pointer_declaration(method_pointer, method_type, method_type),
        pointer_parameter(method_pointer_parameter, method_pointer_end, method_type),
        DebugRecord::Marker(method_pointer_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ];
    records.push(aggregate(method_type, root_pointer, plan.method));
    records.extend(member_records(
        plan.method,
        &method_members,
        method_end,
        |_| Ok(modified_user_type(&[1], callable_return)),
    )?);
    records.extend([
        DebugRecord::Marker(method_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        pointer_declaration(root_pointer, root_type, root_type),
        pointer_parameter(root_pointer_parameter, root_pointer_end, root_type),
        DebugRecord::Marker(root_pointer_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ]);
    records.push(aggregate(root_type, table_array, plan.root));
    records.extend(member_records(
        plan.root,
        &root_members,
        root_end,
        |member| {
            if member.aggregate_tag.as_deref() == Some(plan.method.name.as_str())
                || member.aggregate_tag.as_deref() == plan.method.source_tag.as_deref()
            {
                Ok(modified_user_type(&[1], method_type))
            } else {
                data::member_type_attribute(member.declared_type, None, member.source_fundamental)
            }
        },
    )?);
    records.extend([
        DebugRecord::Marker(root_end),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        array_type(
            table_array,
            table_global,
            u16::try_from(plan.table.array_length.unwrap()).map_err(|_| invalid_plan())?,
            root_type,
        ),
        global(
            plan.table,
            table_global,
            destination_global,
            attribute(
                AttributeName::UserDefinedType,
                AttributeValue::Reference(table_array),
            ),
        ),
        global(
            plan.destination,
            destination_global,
            DebugEntryId(next),
            modified_user_type(&[1, 1], root_type),
        ),
    ]);

    let variables = [
        functions::FunctionVariables {
            global_references: vec![table_global, destination_global],
            ..functions::FunctionVariables::default()
        },
        functions::FunctionVariables {
            global_references: vec![destination_global],
            ..functions::FunctionVariables::default()
        },
    ];
    records.extend(
        functions::selected_plan_with_variables(source_functions, DebugEntryId(next), &variables)?
            .records(unit, layout, &HashMap::new(), None)?,
    );
    Ok(records)
}

fn member_records<F>(
    definition: &AggregateDefinition,
    ids: &[DebugEntryId],
    end: DebugEntryId,
    mut type_attribute: F,
) -> Compilation<Vec<DebugRecord>>
where
    F: FnMut(&mwcc_syntax_trees::AggregateMember) -> Compilation<Attribute>,
{
    definition
        .members
        .iter()
        .enumerate()
        .map(|(index, member)| {
            Ok(entry(
                ids[index],
                Tag::Member,
                vec![
                    sibling(ids.get(index + 1).copied().unwrap_or(end)),
                    attribute(
                        AttributeName::Name,
                        AttributeValue::String(member.name.clone()),
                    ),
                    type_attribute(member)?,
                    attribute(
                        AttributeName::MwMemberFlags,
                        AttributeValue::String(String::new()),
                    ),
                    attribute(
                        AttributeName::Location,
                        AttributeValue::Block2({
                            let mut bytes = vec![0x04];
                            bytes.extend_from_slice(&member.offset.to_be_bytes());
                            bytes.push(0x07);
                            bytes
                        }),
                    ),
                ],
            ))
        })
        .collect()
}

fn aggregate(
    id: DebugEntryId,
    sibling_id: DebugEntryId,
    definition: &AggregateDefinition,
) -> DebugRecord {
    entry(
        id,
        if definition.is_union {
            Tag::UnionType
        } else {
            Tag::StructureType
        },
        vec![
            sibling(sibling_id),
            attribute(
                AttributeName::Name,
                AttributeValue::String(
                    definition
                        .source_tag
                        .clone()
                        .unwrap_or_else(|| definition.name.clone()),
                ),
            ),
            attribute(
                AttributeName::ByteSize,
                AttributeValue::Data4(definition.byte_size),
            ),
        ],
    )
}

fn pointer_declaration(
    id: DebugEntryId,
    sibling_id: DebugEntryId,
    aggregate: DebugEntryId,
) -> DebugRecord {
    entry(
        id,
        Tag::ModifiedType,
        vec![sibling(sibling_id), modified_user_type(&[2], aggregate)],
    )
}

fn pointer_parameter(
    id: DebugEntryId,
    sibling_id: DebugEntryId,
    aggregate: DebugEntryId,
) -> DebugRecord {
    entry(
        id,
        Tag::FormalParameter,
        vec![sibling(sibling_id), modified_user_type(&[2], aggregate)],
    )
}

fn array_type(
    id: DebugEntryId,
    sibling_id: DebugEntryId,
    length: u16,
    aggregate: DebugEntryId,
) -> DebugRecord {
    let mut bytes = vec![0, 0, 10];
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&u32::from(length - 1).to_be_bytes());
    bytes.extend_from_slice(&[8, 0, 0x83, 0, 5, 1]);
    let relocation_offset = bytes.len() as u32;
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    entry(
        id,
        Tag::ArrayType,
        vec![
            sibling(sibling_id),
            attribute(
                AttributeName::SubscriptData,
                AttributeValue::RelocatableBlock2(Block {
                    bytes,
                    relocations: vec![BlockRelocation {
                        offset: relocation_offset,
                        address: Address::debug_entry(aggregate),
                    }],
                }),
            ),
        ],
    )
}

fn global(
    declaration: &GlobalDeclaration,
    id: DebugEntryId,
    sibling_id: DebugEntryId,
    type_attribute: Attribute,
) -> DebugRecord {
    entry(
        id,
        Tag::GlobalVariable,
        vec![
            sibling(sibling_id),
            attribute(
                AttributeName::Name,
                AttributeValue::String(declaration.name.clone()),
            ),
            type_attribute,
            attribute(
                AttributeName::Location,
                AttributeValue::RelocatableBlock2(Block {
                    bytes: vec![0x03, 0, 0, 0, 0],
                    relocations: vec![BlockRelocation {
                        offset: 1,
                        address: Address::external(&declaration.name),
                    }],
                }),
            ),
        ],
    )
}

fn modified_user_type(modifiers: &[u8], target: DebugEntryId) -> Attribute {
    let mut bytes = modifiers.to_vec();
    let relocation_offset = bytes.len() as u32;
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    attribute(
        AttributeName::ModifiedUserDefinedType,
        AttributeValue::RelocatableBlock2(Block {
            bytes,
            relocations: vec![BlockRelocation {
                offset: relocation_offset,
                address: Address::debug_entry(target),
            }],
        }),
    )
}

fn sibling(id: DebugEntryId) -> Attribute {
    attribute(AttributeName::Sibling, AttributeValue::Reference(id))
}

fn entry(id: DebugEntryId, tag: Tag, attributes: Vec<Attribute>) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag,
        attributes,
    })
}

fn allocate(next: &mut u32) -> DebugEntryId {
    let id = DebugEntryId(*next);
    *next += 1;
    id
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid aggregate-pointer registry plan")
}
