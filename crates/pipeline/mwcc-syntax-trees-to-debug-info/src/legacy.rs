//! Measured grouped DWARF-1 emitted by the 2.3.x and early 2.4.x compilers.

mod captures;
mod data;
mod functions;

use super::convert_relocations;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugInfo, DebugRecord, FundamentalType, LineRecord, LineTable, Tag,
};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{layout_function_placements, DebugLayout, DebugSections, FunctionPlacement};
use mwcc_syntax_trees::{AsmItem, Expression, Function, TranslationUnit, Type};
use mwcc_versions::CompilerBuild;

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
    /// A functionless translation unit containing supported scalar, array, and
    /// aggregate data declarations.
    DataOnly,
    /// Aggregate data followed by no-frame inline-asm functions. Each written
    /// asm instruction has an exact source line and emits one machine word.
    VerbatimAsmWithData,
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
    let shape = classify_shape(unit, machine_functions, &globals, build)?;

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
            .ok_or_else(|| {
                Diagnostic::error("debug-info: physical source provenance is required")
            })?;
        source_functions.push((&unit.functions[index], source));
    }

    let mut line_records = Vec::with_capacity(machine_functions.len() + 1);
    if shape == MeasuredShape::VerbatimAsmWithData {
        for (machine_index, (function, _)) in source_functions.iter().enumerate() {
            let mut address = layout.offsets[machine_index];
            for item in function
                .asm_body
                .as_ref()
                .expect("verbatim-asm shape has an asm body")
            {
                if let AsmItem::Instruction(instruction) = item {
                    if matches!(instruction.mnemonic.as_str(), "nofralloc" | "frfree") {
                        continue;
                    }
                    line_records.push(LineRecord {
                        line: instruction.source_line,
                        column: u16::MAX,
                        address_delta: address,
                    });
                    address += 4;
                }
            }
            if address != layout.offsets[machine_index] + layout.sizes[machine_index] {
                return Err(Diagnostic::error(format!(
                    "debug-info: inline-asm source map for '{}' does not cover its emitted text",
                    function.name
                )));
            }
        }
    } else {
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
            attribute(
                AttributeName::Name,
                AttributeValue::String(source_name.into()),
            ),
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

    if shape == MeasuredShape::DataOnly {
        let mut records: Vec<_> = entries.into_iter().map(DebugRecord::Entry).collect();
        records.extend(data::records(unit, &globals, first_global_id, false)?.records);
        return finish(line, records, data_only_layout(build));
    }

    if shape == MeasuredShape::VerbatimAsmWithData {
        let mut records: Vec<_> = entries.into_iter().map(DebugRecord::Entry).collect();
        let data = data::records(unit, &globals, first_global_id, true)?;
        records.extend(data.records);
        records.extend(functions::records(
            unit,
            &source_functions
                .iter()
                .map(|(function, _)| *function)
                .collect::<Vec<_>>(),
            &layout,
            data.next_id,
            &data.aggregate_ids,
        )?);
        return finish(line, records, DebugLayout::AfterDataGrouped);
    }

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

    let mut records: Vec<_> = entries.into_iter().map(DebugRecord::Entry).collect();
    match shape {
        MeasuredShape::BasicParameter => records.extend([
            DebugRecord::Marker(PARAMETER_END),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
            DebugRecord::Marker(FUNCTION_END),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
        ]),
        MeasuredShape::ConstantFunctions => records.extend([
            DebugRecord::Marker(FUNCTION_END),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
            DebugRecord::Raw(vec![0, 0, 0, 4]),
        ]),
        MeasuredShape::DataOnly => unreachable!("data-only units return before function records"),
        MeasuredShape::VerbatimAsmWithData => {
            unreachable!("verbatim asm/data units return before legacy function records")
        }
    }
    finish(line, records, DebugLayout::BeforeDataGrouped)
}

/// Legacy compilers place a functionless unit's DWARF sections before its data.
/// Fragmented 4.x generations keep the monolithic data-only payload but move it
/// after ordinary data, independently of the DIE encoding itself.
fn data_only_layout(build: CompilerBuild) -> DebugLayout {
    if build.version.0 >= 4 {
        DebugLayout::AfterDataGrouped
    } else {
        DebugLayout::BeforeDataGrouped
    }
}

pub(super) fn lookup_capture(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    build: CompilerBuild,
) -> Compilation<Option<DebugSections>> {
    captures::lookup(unit, machine_functions, source_name, build)
}

fn finish(
    line: mwcc_dwarf1::EncodedSection,
    records: Vec<DebugRecord>,
    layout: DebugLayout,
) -> Compilation<DebugSections> {
    let mut debug_model = DebugInfo { records };
    // MWCC aligns the logical end of `.debug` with a final null record whose
    // declared length includes the required zero fill. It is absent when the
    // structural terminators already end on a four-byte boundary.
    let unpadded_len = debug_model.encode().bytes.len();
    let padding = (4 - unpadded_len % 4) % 4;
    if padding != 0 {
        let record_len = 4 + padding;
        let mut record = vec![0, 0, 0, record_len as u8];
        record.resize(record_len, 0);
        debug_model.records.push(DebugRecord::Raw(record));
    }
    debug_model.records.push(DebugRecord::Marker(UNIT_END));
    let encoded = debug_model.encode_with_offsets();
    let offsets = encoded.entry_offsets.into_iter().collect();

    Ok(DebugSections {
        layout,
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
    build: CompilerBuild,
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

    if unit.functions.is_empty() && machine_functions.is_empty() && !globals.is_empty() {
        return Ok(MeasuredShape::DataOnly);
    }

    let verbatim_asm_with_data = build.version == (2, 4, 2)
        && build.build == 81
        && !globals.is_empty()
        && !unit.functions.is_empty()
        && unit.functions.len() == machine_functions.len()
        && unit.functions.iter().all(|function| {
            !function.is_static
                && function.return_type == Type::Void
                && function
                    .parameters
                    .iter()
                    .all(|parameter| matches!(parameter.parameter_type, Type::StructPointer { .. }))
                && function.asm_body.as_ref().is_some_and(|body| {
                    body.iter().any(|item| {
                        matches!(item, AsmItem::Instruction(instruction) if instruction.mnemonic == "nofralloc")
                    })
                })
        });
    if verbatim_asm_with_data {
        return Ok(MeasuredShape::VerbatimAsmWithData);
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
        && matches!(
            function.return_expression,
            Some(Expression::IntegerLiteral(_))
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_only_layout_changes_at_the_fragmented_generation() {
        let legacy =
            mwcc_versions::by_label_experimental("GC/1.2.5").expect("legacy build");
        let fragmented =
            mwcc_versions::by_label_experimental("Wii/1.0").expect("fragmented build");
        assert_eq!(data_only_layout(legacy), DebugLayout::BeforeDataGrouped);
        assert_eq!(data_only_layout(fragmented), DebugLayout::AfterDataGrouped);
    }
}
