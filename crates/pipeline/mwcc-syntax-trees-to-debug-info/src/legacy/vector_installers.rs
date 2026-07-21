//! Legacy DWARF plan for a no-frame exception vector and its installer.

use super::{attribute, FUNCTION_END};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, AttributeName, AttributeValue, DebugEntry, DebugEntryId, DebugRecord, FundamentalType,
    LineRecord, Tag,
};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    AsmItem, Expression, Function, FunctionSource, Statement, TranslationUnit, Type,
};

const LOCAL_END: DebugEntryId = DebugEntryId(u32::MAX - 3);

pub(super) fn matches(unit: &TranslationUnit, machine_functions: &[MachineFunction]) -> bool {
    if unit.functions.len() != 2 || machine_functions.len() != 2 {
        return false;
    }
    let [vector, installer] = unit.functions.as_slice() else {
        return false;
    };
    vector.is_static
        && vector.return_type == Type::Void
        && vector.parameters.is_empty()
        && vector.asm_body.as_ref().is_some_and(|body| {
            body.iter().any(|item| {
                matches!(item, AsmItem::Instruction(instruction) if instruction.mnemonic == "nofralloc")
            })
        })
        && installer_shape(installer)
}

fn installer_shape(function: &Function) -> bool {
    if function.is_static
        || function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return false;
    }
    let [local] = function.locals.as_slice() else {
        return false;
    };
    if !matches!(
        local.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return false;
    }
    let [copy, flush, barrier, invalidate] = function.statements.as_slice() else {
        return false;
    };
    let calls = [copy, flush, barrier, invalidate].map(|statement| match statement {
        Statement::Expression(Expression::Call { name, arguments }) => {
            Some((name.as_str(), arguments.as_slice()))
        }
        _ => None,
    });
    let [Some((_, copy_arguments)), Some((_, flush_arguments)), Some((barrier, barrier_arguments)), Some((_, invalidate_arguments))] =
        calls
    else {
        return false;
    };
    matches!(copy_arguments.first(), Some(Expression::Variable(name)) if name == &local.name)
        && matches!(flush_arguments.first(), Some(Expression::Variable(name)) if name == &local.name)
        && matches!(invalidate_arguments.first(), Some(Expression::Variable(name)) if name == &local.name)
        && barrier == "__sync"
        && barrier_arguments.is_empty()
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    let [(vector, _), (_installer, source)] = functions else {
        return Err(invalid_plan());
    };
    let [vector_machine, installer_machine] = machine_functions else {
        return Err(invalid_plan());
    };
    let mut records = Vec::new();
    let mut address = layout.offsets[0];
    for item in vector.asm_body.as_ref().ok_or_else(invalid_plan)? {
        if let AsmItem::Instruction(instruction) = item {
            if matches!(instruction.mnemonic.as_str(), "nofralloc" | "frfree") {
                continue;
            }
            records.push(record(instruction.source_line, address));
            address += 4;
        }
    }
    if address != layout.offsets[0] + layout.sizes[0]
        || vector_machine.encode_text().len() as u32 != layout.sizes[0]
    {
        return Err(invalid_plan());
    }

    let local_line = match source.local_lines.as_slice() {
        [Some(line)] => *line,
        _ => return Err(invalid_plan()),
    };
    let [_, flush_line, barrier_line, invalidate_line] = source.statement_lines.as_slice() else {
        return Err(invalid_plan());
    };
    let first_address = installer_machine
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediateShifted { d: 5, a: 0, .. }
            )
        })
        .ok_or_else(invalid_plan)? as u32;
    let calls = installer_machine
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::BranchAndLink { .. }).then_some(index as u32)
        })
        .collect::<Vec<_>>();
    let [copy_call, flush_call, invalidate_call] = calls.as_slice() else {
        return Err(invalid_plan());
    };
    let barrier = installer_machine
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::Synchronize))
        .ok_or_else(invalid_plan)? as u32;
    let start = layout.offsets[1];
    records.extend([
        record(source.body_start_line, start),
        record(local_line, start + first_address * 4),
        record(*flush_line, start + (*copy_call + 1) * 4),
        record(*barrier_line, start + barrier * 4),
        record(*invalidate_line, start + (barrier + 1) * 4),
        record(source.body_end_line, start + (*invalidate_call + 1) * 4),
    ]);
    if *flush_call <= *copy_call || barrier <= *flush_call || *invalidate_call <= barrier {
        return Err(invalid_plan());
    }
    Ok(records)
}

pub(super) fn records(
    functions: &[&Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    let [vector, installer] = functions else {
        return Err(invalid_plan());
    };
    let vector_id = first_id;
    let installer_id = DebugEntryId(first_id.0 + 1);
    let local_id = DebugEntryId(first_id.0 + 2);
    let local = installer.locals.first().ok_or_else(invalid_plan)?;

    Ok(vec![
        DebugRecord::Entry(DebugEntry {
            id: vector_id,
            tag: Tag::LocalSubroutine,
            attributes: function_attributes(vector, installer_id, layout, 0),
        }),
        DebugRecord::Entry(DebugEntry {
            id: installer_id,
            tag: Tag::GlobalSubroutine,
            attributes: function_attributes(installer, FUNCTION_END, layout, 1),
        }),
        DebugRecord::Entry(DebugEntry {
            id: local_id,
            tag: Tag::LocalVariable,
            attributes: vec![
                attribute(AttributeName::Sibling, AttributeValue::Reference(LOCAL_END)),
                attribute(
                    AttributeName::Name,
                    AttributeValue::String(local.name.clone()),
                ),
                attribute(
                    AttributeName::FundamentalType,
                    AttributeValue::Data2(FundamentalType::Pointer as u16),
                ),
                attribute(
                    AttributeName::Location,
                    AttributeValue::Block2(vec![1, 0, 0, 0, 31]),
                ),
            ],
        }),
        DebugRecord::Marker(LOCAL_END),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Marker(FUNCTION_END),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ])
}

fn function_attributes(
    function: &Function,
    sibling: DebugEntryId,
    layout: &FunctionLayout,
    index: usize,
) -> Vec<mwcc_dwarf1::Attribute> {
    vec![
        attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
        attribute(
            AttributeName::Name,
            AttributeValue::String(function.name.clone()),
        ),
        attribute(
            AttributeName::LowPc,
            AttributeValue::Address(Address::external(&function.name)),
        ),
        attribute(
            AttributeName::HighPc,
            AttributeValue::Address(Address::external_with_addend(
                ".text",
                (layout.offsets[index] + layout.sizes[index]) as i32,
            )),
        ),
    ]
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid vector-installer plan")
}
