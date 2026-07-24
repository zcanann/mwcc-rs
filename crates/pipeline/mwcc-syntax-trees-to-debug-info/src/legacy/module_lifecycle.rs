//! Debug plan for REL module lifecycle entry points.
//!
//! The legacy compiler interleaves the constructor/destructor array DIEs with
//! the functions that consume them. This differs from the ordinary
//! data-before-functions layout, so keep the recognition, source map, and DIE
//! ordering together.

use super::{attribute, FUNCTION_END};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry, DebugEntryId,
    DebugRecord, FundamentalType, LineRecord, Tag,
};
use mwcc_machine_code::{MachineFunction, RelocationKind, RelocationTarget};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    Expression, Function, FunctionSource, GlobalDeclaration, Statement, TranslationUnit, Type,
};
use mwcc_versions::CompilerBuild;

pub(super) fn matches(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    emitted_globals: &[&GlobalDeclaration],
    build: CompilerBuild,
) -> bool {
    if build.version != (2, 4, 2)
        || build.build != 81
        || !emitted_globals.is_empty()
        || unit.functions.len() != 3
        || machine_functions.len() != 3
        || !unit
            .functions
            .iter()
            .zip(machine_functions)
            .all(|(source, machine)| source.name == machine.name && function_shape(source))
    {
        return false;
    }

    let Some(first_global) = lifecycle_global(&unit.functions[0], 0) else {
        return false;
    };
    let Some(second_global) = lifecycle_global(&unit.functions[1], 1) else {
        return false;
    };
    first_global != second_global
        && call_argument_count(&unit.functions[0]) == Some([1, 0])
        && call_argument_count(&unit.functions[1]) == Some([0, 1])
        && call_argument_count(&unit.functions[2]) == Some([0, 0])
        && unit.globals.iter().all(|global| {
            ![first_global, second_global].contains(&global.name.as_str())
                || (global.is_extern
                    && unit.global_function_types.contains_key(global.name.as_str()))
        })
        && [first_global, second_global].iter().all(|name| {
            unit.globals.iter().any(|global| global.name == *name)
                && unit.global_function_types.contains_key(*name)
        })
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    if functions.len() != 3
        || machine_functions.len() != 3
        || layout.offsets.len() != 3
        || layout.sizes.len() != 3
    {
        return Err(invalid_plan());
    }

    let mut records = Vec::new();
    for (index, ((function, source), machine)) in
        functions.iter().zip(machine_functions).enumerate()
    {
        let start = layout.offsets[index];
        records.push(record(source.body_start_line, start));
        let calls = direct_call_relocations(machine);
        if calls.len() != function.statements.len()
            || source.statement_lines.len() != function.statements.len()
        {
            return Err(invalid_plan());
        }
        let mut previous_call = 0;
        for (statement_index, ((statement, line), call_index)) in function
            .statements
            .iter()
            .zip(&source.statement_lines)
            .zip(calls)
            .enumerate()
        {
            let arguments = statement_call_arguments(statement).ok_or_else(invalid_plan)?;
            let statement_start = match arguments {
                [] => call_index,
                [Expression::Variable(global)] => machine
                    .relocations
                    .iter()
                    .filter(|relocation| {
                        relocation.instruction_index >= previous_call
                            && relocation.instruction_index < call_index
                            && matches!(
                                &relocation.target,
                                RelocationTarget::External(name) if name == global
                            )
                    })
                    .map(|relocation| relocation.instruction_index)
                    .min()
                    .ok_or_else(invalid_plan)?,
                _ => return Err(invalid_plan()),
            };
            records.push(record(*line, start + instruction_offset(statement_start)?));
            previous_call = call_index + usize::from(statement_index + 1 < function.statements.len());
        }
        let final_call = direct_call_relocations(machine)
            .last()
            .copied()
            .ok_or_else(invalid_plan)?;
        records.push(record(
            source.body_end_line,
            start + instruction_offset(final_call + 1)?,
        ));
    }
    Ok(records)
}

pub(super) fn records(
    unit: &TranslationUnit,
    functions: &[&Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    let [prolog, epilog, unresolved] = functions else {
        return Err(invalid_plan());
    };
    if layout.offsets.len() != 3 || layout.sizes.len() != 3 {
        return Err(invalid_plan());
    }
    let constructors = lifecycle_global(prolog, 0).ok_or_else(invalid_plan)?;
    let destructors = lifecycle_global(epilog, 1).ok_or_else(invalid_plan)?;
    if ![constructors, destructors]
        .iter()
        .all(|name| unit.global_function_types.contains_key(*name))
    {
        return Err(invalid_plan());
    }

    let void_type = first_id;
    let constructors_array = DebugEntryId(first_id.0 + 1);
    let constructors_global = DebugEntryId(first_id.0 + 2);
    let prolog_id = DebugEntryId(first_id.0 + 3);
    let destructors_array = DebugEntryId(first_id.0 + 4);
    let destructors_global = DebugEntryId(first_id.0 + 5);
    let epilog_id = DebugEntryId(first_id.0 + 6);
    let unresolved_id = DebugEntryId(first_id.0 + 7);

    Ok(vec![
        DebugRecord::Entry(DebugEntry {
            id: void_type,
            tag: Tag::ModifiedType,
            attributes: vec![
                attribute(
                    AttributeName::Sibling,
                    AttributeValue::Reference(constructors_array),
                ),
                attribute(
                    AttributeName::FundamentalType,
                    AttributeValue::Data2(FundamentalType::Void as u16),
                ),
            ],
        }),
        callable_array(constructors_array, constructors_global, void_type),
        external_array(
            constructors_global,
            prolog_id,
            constructors,
            constructors_array,
        ),
        function(prolog_id, destructors_array, prolog, layout, 0, Some(constructors_global)),
        callable_array(destructors_array, destructors_global, void_type),
        external_array(
            destructors_global,
            epilog_id,
            destructors,
            destructors_array,
        ),
        function(epilog_id, unresolved_id, epilog, layout, 1, Some(destructors_global)),
        function(
            unresolved_id,
            FUNCTION_END,
            unresolved,
            layout,
            2,
            None,
        ),
        DebugRecord::Marker(FUNCTION_END),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ])
}

fn function_shape(function: &Function) -> bool {
    !function.is_static
        && function.return_type == Type::Void
        && function.parameters.is_empty()
        && function.locals.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
        && function.statements.iter().all(|statement| {
            statement_call_arguments(statement).is_some_and(|arguments| {
                arguments.is_empty()
                    || matches!(arguments, [Expression::Variable(_)])
            })
        })
}

fn call_argument_count(function: &Function) -> Option<[usize; 2]> {
    let first = statement_call_arguments(function.statements.first()?)?.len();
    let second = function
        .statements
        .get(1)
        .and_then(statement_call_arguments)
        .map_or(0, <[_]>::len);
    (function.statements.len() <= 2).then_some([first, second])
}

fn lifecycle_global(function: &Function, statement_index: usize) -> Option<&str> {
    match statement_call_arguments(function.statements.get(statement_index)?)? {
        [Expression::Variable(name)] => Some(name),
        _ => None,
    }
}

fn statement_call_arguments(statement: &Statement) -> Option<&[Expression]> {
    match statement {
        Statement::Expression(Expression::Call { arguments, .. }) => Some(arguments),
        _ => None,
    }
}

fn direct_call_relocations(machine: &MachineFunction) -> Vec<usize> {
    machine
        .relocations
        .iter()
        .filter(|relocation| relocation.kind == RelocationKind::Rel24)
        .map(|relocation| relocation.instruction_index)
        .collect()
}

fn callable_array(
    id: DebugEntryId,
    sibling: DebugEntryId,
    element_type: DebugEntryId,
) -> DebugRecord {
    let mut bytes = vec![0, 0, 10];
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[8, 0, 0x83, 0, 6, 3, 1]);
    let relocation_offset = bytes.len() as u32;
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::ArrayType,
        attributes: vec![
            attribute(
                AttributeName::Sibling,
                AttributeValue::Reference(sibling),
            ),
            attribute(
                AttributeName::SubscriptData,
                AttributeValue::RelocatableBlock2(Block {
                    bytes,
                    relocations: vec![BlockRelocation {
                        offset: relocation_offset,
                        address: Address::debug_entry(element_type),
                    }],
                }),
            ),
        ],
    })
}

fn external_array(
    id: DebugEntryId,
    sibling: DebugEntryId,
    name: &str,
    array_type: DebugEntryId,
) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::GlobalVariable,
        attributes: vec![
            attribute(
                AttributeName::Sibling,
                AttributeValue::Reference(sibling),
            ),
            attribute(AttributeName::Name, AttributeValue::String(name.into())),
            attribute(
                AttributeName::UserDefinedType,
                AttributeValue::Reference(array_type),
            ),
            attribute(
                AttributeName::Location,
                AttributeValue::RelocatableBlock2(Block {
                    bytes: vec![3, 0, 0, 0, 0],
                    relocations: vec![BlockRelocation {
                        offset: 1,
                        address: Address::external(name),
                    }],
                }),
            ),
        ],
    })
}

fn function(
    id: DebugEntryId,
    sibling: DebugEntryId,
    source: &Function,
    layout: &FunctionLayout,
    index: usize,
    referenced_global: Option<DebugEntryId>,
) -> DebugRecord {
    let mut attributes = vec![
        attribute(
            AttributeName::Sibling,
            AttributeValue::Reference(sibling),
        ),
        attribute(
            AttributeName::Name,
            AttributeValue::String(source.name.clone()),
        ),
        attribute(
            AttributeName::LowPc,
            AttributeValue::Address(Address::external(&source.name)),
        ),
        attribute(
            AttributeName::HighPc,
            AttributeValue::Address(Address::external_with_addend(
                ".text",
                (layout.offsets[index] + layout.sizes[index]) as i32,
            )),
        ),
    ];
    if let Some(global) = referenced_global {
        attributes.push(attribute(
            AttributeName::MwVtableElement,
            AttributeValue::Reference(global),
        ));
    }
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::GlobalSubroutine,
        attributes,
    })
}

fn instruction_offset(index: usize) -> Compilation<u32> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_mul(4))
        .ok_or_else(invalid_plan)
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid module lifecycle plan")
}

#[cfg(test)]
mod tests;
