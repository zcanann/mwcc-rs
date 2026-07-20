//! Measured grouped DWARF-1 emitted by the 2.3.x and early 2.4.x compilers.

use super::convert_relocations;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugInfo, FundamentalType, LineRecord, LineTable, Tag,
};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{
    layout_function_placements, DebugLayout, DebugSections, FunctionPlacement,
};
use mwcc_syntax_trees::{Expression, Function, TranslationUnit, Type};
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

const COMPILE_UNIT: DebugEntryId = DebugEntryId(0);
const PARAMETER_END: DebugEntryId = DebugEntryId(u32::MAX - 2);
const FUNCTION_END: DebugEntryId = DebugEntryId(u32::MAX - 1);
const UNIT_END: DebugEntryId = DebugEntryId(u32::MAX);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeasuredShape {
    /// One exported signed-int global and one signed-int function using one
    /// signed-int parameter (canary 1239).
    BasicParameter,
    /// Exported leaf functions whose optimized bodies are constant returns.
    /// Parameters are absent from the DIE stream because none remain live.
    ConstantFunctions,
}

pub(super) fn lower(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    build: CompilerBuild,
    code_alignment: u32,
) -> Compilation<DebugSections> {
    let globals: Vec<_> = unit
        .globals
        .iter()
        .filter(|global| !global.is_extern && !global.is_static && !global.name.is_empty())
        .collect();
    let shape = classify_shape(unit, machine_functions, &globals)?;

    let placements: Vec<FunctionPlacement> = machine_functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.encode_text().len() as u32,
            deferred: function.text_deferred,
        })
        .collect();
    let layout = layout_function_placements(&placements, code_alignment);

    // Deferred unit scheduling may reorder machine functions after parsing.
    // Resolve source coordinates and types by name, then emit line/DIE records
    // in final text-layout order.
    let mut source_functions = Vec::with_capacity(machine_functions.len());
    for machine in machine_functions {
        let index = unit
            .functions
            .iter()
            .position(|function| function.name == machine.name)
            .ok_or_else(|| {
                Diagnostic::error("debug-info: an emitted function has no source declaration")
            })?;
        let source = unit
            .function_sources
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| Diagnostic::error("debug-info: physical source provenance is required"))?;
        source_functions.push((&unit.functions[index], source));
    }

    let mut line_records = Vec::with_capacity(machine_functions.len() + 1);
    for (machine_index, (_, source)) in source_functions.iter().enumerate() {
        let terminal_return_line = source.terminal_return_line.ok_or_else(|| {
            Diagnostic::error(
                "debug-info: a legacy function without a terminal return is not implemented yet (roadmap)",
            )
        })?;
        line_records.push(LineRecord {
            line: if build.version == (2, 3, 3) {
                source.body_start_line
            } else {
                terminal_return_line
            },
            column: u16::MAX,
            address_delta: layout.offsets[machine_index],
        });
    }
    line_records.push(LineRecord {
        line: 0,
        column: u16::MAX,
        address_delta: layout.byte_len,
    });
    let line = LineTable {
        base_address: Address::external(".text"),
        records: line_records,
    }
    .encode();

    let first_global_id = DebugEntryId(1);
    let first_function_id = DebugEntryId(1 + globals.len() as u32);
    let parameter_id = DebugEntryId(first_function_id.0 + machine_functions.len() as u32);
    let mut entries = Vec::new();
    entries.push(DebugEntry {
        id: COMPILE_UNIT,
        tag: Tag::CompileUnit,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(UNIT_END)),
            attribute(
                AttributeName::Producer,
                AttributeValue::String("MW EABI PPC C-Compiler".into()),
            ),
            attribute(AttributeName::Name, AttributeValue::String(source_name.into())),
            attribute(AttributeName::Language, AttributeValue::Data4(1)),
            attribute(
                AttributeName::LowPc,
                AttributeValue::Address(Address::external(".text")),
            ),
            attribute(
                AttributeName::HighPc,
                AttributeValue::Address(Address::external_with_addend(
                    ".text",
                    layout.byte_len as i32,
                )),
            ),
            attribute(
                AttributeName::StatementList,
                AttributeValue::Data4Address(Address::external(".line")),
            ),
        ],
    });

    for (index, global) in globals.iter().enumerate() {
        let next = if index + 1 < globals.len() {
            DebugEntryId(first_global_id.0 + index as u32 + 1)
        } else {
            first_function_id
        };
        entries.push(DebugEntry {
            id: DebugEntryId(first_global_id.0 + index as u32),
            tag: Tag::GlobalVariable,
            attributes: vec![
                attribute(AttributeName::Sibling, AttributeValue::Reference(next)),
                attribute(
                    AttributeName::Name,
                    AttributeValue::String(global.name.clone()),
                ),
                signed_int_type(),
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
        });
    }

    for (index, (function, _)) in source_functions.iter().enumerate() {
        let function_id = DebugEntryId(first_function_id.0 + index as u32);
        let next_function = if index + 1 < source_functions.len() {
            DebugEntryId(function_id.0 + 1)
        } else {
            FUNCTION_END
        };
        entries.push(DebugEntry {
            id: function_id,
            tag: Tag::GlobalSubroutine,
            attributes: vec![
                attribute(
                    AttributeName::Sibling,
                    AttributeValue::Reference(next_function),
                ),
                attribute(
                    AttributeName::Name,
                    AttributeValue::String(function.name.clone()),
                ),
                signed_int_type(),
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
            ],
        });

        if shape == MeasuredShape::BasicParameter {
            let parameter = &function.parameters[0];
            entries.push(DebugEntry {
                id: parameter_id,
                tag: Tag::FormalParameter,
                attributes: vec![
                    attribute(
                        AttributeName::Sibling,
                        AttributeValue::Reference(PARAMETER_END),
                    ),
                    attribute(
                        AttributeName::Name,
                        AttributeValue::String(parameter.name.clone()),
                    ),
                    signed_int_type(),
                    attribute(
                        AttributeName::Location,
                        AttributeValue::Block2(vec![0x01, 0, 0, 0, 3]),
                    ),
                ],
            });
        }
    }

    let terminal_records = match shape {
        MeasuredShape::BasicParameter => vec![
            vec![0, 0, 0, 4],
            vec![0, 0, 0, 4],
            vec![0, 0, 0, 4],
        ],
        MeasuredShape::ConstantFunctions => vec![vec![0, 0, 0, 4], vec![0, 0, 0, 4]],
    };
    let mut debug_model = DebugInfo {
        entries,
        terminal_records,
    };
    // MWCC aligns the logical end of `.debug` with a final null record whose
    // declared length includes the required zero fill. It is absent when the
    // structural terminators already end on a four-byte boundary.
    let unpadded_len = debug_model.encode().bytes.len();
    let padding = (4 - unpadded_len % 4) % 4;
    if padding != 0 {
        let record_len = 4 + padding;
        let mut record = vec![0, 0, 0, record_len as u8];
        record.resize(record_len, 0);
        debug_model.terminal_records.push(record);
    }
    let terminal_len: usize = debug_model.terminal_records.iter().map(Vec::len).sum();
    let encoded = debug_model.encode_with_offsets();
    let entries_end = encoded
        .section
        .bytes
        .len()
        .checked_sub(terminal_len)
        .expect("the measured legacy terminal records have fixed sizes") as u32;
    let mut offsets: HashMap<DebugEntryId, u32> = encoded.entry_offsets.into_iter().collect();
    match shape {
        MeasuredShape::BasicParameter => {
            offsets.insert(PARAMETER_END, entries_end);
            offsets.insert(FUNCTION_END, entries_end + 4);
        }
        MeasuredShape::ConstantFunctions => {
            offsets.insert(FUNCTION_END, entries_end);
        }
    }
    offsets.insert(UNIT_END, encoded.section.bytes.len() as u32);

    Ok(DebugSections {
        layout: DebugLayout::BeforeDataGrouped,
        line: line.bytes,
        debug: encoded.section.bytes,
        line_relocations: convert_relocations(line.relocations, &offsets, false),
        debug_relocations: convert_relocations(encoded.section.relocations, &offsets, true),
        symbols: Vec::new(),
    })
}

fn classify_shape(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    globals: &[&mwcc_syntax_trees::GlobalDeclaration],
) -> Compilation<MeasuredShape> {
    let basic_parameter = globals.len() == 1
        && unit.functions.len() == 1
        && machine_functions.len() == 1
        && globals[0].declared_type == Type::Int
        && unit.functions[0].return_type == Type::Int
        && unit.functions[0].parameters.len() == 1
        && unit.functions[0].parameters[0].parameter_type == Type::Int;
    if basic_parameter {
        return Ok(MeasuredShape::BasicParameter);
    }

    let constant_functions = globals.is_empty()
        && !unit.functions.is_empty()
        && unit.functions.len() == machine_functions.len()
        && unit.functions.iter().all(is_exported_constant_int_function);
    if constant_functions {
        return Ok(MeasuredShape::ConstantFunctions);
    }

    Err(Diagnostic::error(
        "debug-info: this translation unit's legacy DWARF DIE shape is not implemented yet (roadmap)",
    ))
}

fn is_exported_constant_int_function(function: &Function) -> bool {
    !function.is_static
        && function.return_type == Type::Int
        && function.locals.is_empty()
        && function.statements.is_empty()
        && function.guards.is_empty()
        && matches!(function.return_expression, Some(Expression::IntegerLiteral(_)))
}

fn attribute(name: AttributeName, value: AttributeValue) -> Attribute {
    Attribute { name, value }
}

fn signed_int_type() -> Attribute {
    attribute(
        AttributeName::FundamentalType,
        AttributeValue::Data2(FundamentalType::SignedInteger as u16),
    )
}
