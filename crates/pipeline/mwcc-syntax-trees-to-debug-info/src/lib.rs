//! Pipeline: parsed source + finalized machine functions -> CodeWarrior DWARF 1.
//!
//! The syntax tree supplies names, types, and physical source provenance; the
//! machine representation supplies final code sizes and deferred layout state.
//! DWARF byte encoding and ELF container policy remain in their own crates.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, Attribute, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry,
    DebugEntryId, DebugInfo, FundamentalType, LineRecord, LineTable, RelocationTarget, Tag,
};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{
    layout_function_placements, DebugLayout, DebugRelocation, DebugRelocationKind,
    DebugRelocationTarget, DebugSections, FunctionPlacement,
};
use mwcc_syntax_trees::{TranslationUnit, Type};
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

const COMPILE_UNIT: DebugEntryId = DebugEntryId(0);
const GLOBAL: DebugEntryId = DebugEntryId(1);
const FUNCTION: DebugEntryId = DebugEntryId(2);
const PARAMETER: DebugEntryId = DebugEntryId(3);
const PARAMETER_END: DebugEntryId = DebugEntryId(100);
const FUNCTION_END: DebugEntryId = DebugEntryId(101);
const UNIT_END: DebugEntryId = DebugEntryId(102);

/// Lower the first measured legacy DWARF shape. Unsupported debug shapes remain
/// explicit deferrals until characterized; ordinary compilation is unaffected.
pub fn lower_debug_info(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    source_name: &str,
    build: CompilerBuild,
    code_alignment: u32,
) -> Compilation<DebugSections> {
    if build.version.0 >= 4 || (build.version == (2, 4, 2) && build.build == 81) {
        return Err(Diagnostic::error(
            "this compiler generation's fragmented/interleaved debug-object format is not implemented yet (roadmap)",
        ));
    }

    let globals: Vec<_> = unit
        .globals
        .iter()
        .filter(|global| !global.is_extern && !global.is_static && !global.name.is_empty())
        .collect();
    if globals.len() != 1
        || unit.functions.len() != 1
        || machine_functions.len() != 1
        || globals[0].declared_type != Type::Int
        || unit.functions[0].return_type != Type::Int
        || unit.functions[0].parameters.len() != 1
        || unit.functions[0].parameters[0].parameter_type != Type::Int
    {
        return Err(Diagnostic::error(
            "this translation unit's legacy DWARF DIE shape is not implemented yet (roadmap)",
        ));
    }
    let source = unit
        .function_sources
        .first()
        .copied()
        .flatten()
        .ok_or_else(|| Diagnostic::error("debug information requires physical source provenance"))?;
    let terminal_return_line = source.terminal_return_line.ok_or_else(|| {
        Diagnostic::error("legacy debug information for a function without a terminal return is not implemented yet (roadmap)")
    })?;
    let statement_line = if build.version == (2, 3, 3) {
        source.body_start_line
    } else {
        terminal_return_line
    };

    let placements: Vec<FunctionPlacement> = machine_functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.encode_text().len() as u32,
            deferred: function.text_deferred,
        })
        .collect();
    let layout = layout_function_placements(&placements, code_alignment);
    let function_offset = layout.offsets[0];
    let function_size = layout.sizes[0];

    let line = LineTable {
        base_address: Address::external(".text"),
        records: vec![
            LineRecord {
                line: statement_line,
                column: u16::MAX,
                address_delta: function_offset,
            },
            LineRecord {
                line: 0,
                column: u16::MAX,
                address_delta: function_offset + function_size,
            },
        ],
    }
    .encode();

    let global = globals[0];
    let function = &unit.functions[0];
    let parameter = &function.parameters[0];
    let debug_model = DebugInfo {
        entries: vec![
            DebugEntry {
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
            },
            DebugEntry {
                id: GLOBAL,
                tag: Tag::GlobalVariable,
                attributes: vec![
                    attribute(AttributeName::Sibling, AttributeValue::Reference(FUNCTION)),
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
            },
            DebugEntry {
                id: FUNCTION,
                tag: Tag::GlobalSubroutine,
                attributes: vec![
                    attribute(
                        AttributeName::Sibling,
                        AttributeValue::Reference(FUNCTION_END),
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
                            (function_offset + function_size) as i32,
                        )),
                    ),
                ],
            },
            DebugEntry {
                id: PARAMETER,
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
            },
        ],
        terminal_records: vec![
            vec![0, 0, 0, 4],
            vec![0, 0, 0, 4],
            vec![0, 0, 0, 4],
            vec![0, 0, 0, 6, 0, 0],
        ],
    };
    let encoded = debug_model.encode_with_offsets();
    let entries_end = encoded
        .section
        .bytes
        .len()
        .checked_sub(18)
        .expect("the legacy terminal records are fixed-size") as u32;
    let mut offsets: HashMap<DebugEntryId, u32> = encoded.entry_offsets.into_iter().collect();
    offsets.insert(PARAMETER_END, entries_end);
    offsets.insert(FUNCTION_END, entries_end + 4);
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

fn attribute(name: AttributeName, value: AttributeValue) -> Attribute {
    Attribute { name, value }
}

fn signed_int_type() -> Attribute {
    attribute(
        AttributeName::FundamentalType,
        AttributeValue::Data2(FundamentalType::SignedInteger as u16),
    )
}

fn convert_relocations(
    relocations: Vec<mwcc_dwarf1::Relocation>,
    debug_offsets: &HashMap<DebugEntryId, u32>,
    first_reference_uses_aligned_relocation: bool,
) -> Vec<DebugRelocation> {
    relocations
        .into_iter()
        .map(|relocation| {
            let aligned = first_reference_uses_aligned_relocation && relocation.offset == 8;
            let (target, target_addend) = match relocation.target {
                RelocationTarget::External(name) if name.starts_with('.') => {
                    (DebugRelocationTarget::Section(name), 0)
                }
                RelocationTarget::External(name) => {
                    (DebugRelocationTarget::Symbol(name), 0)
                }
                RelocationTarget::DebugEntry(id) => (
                    DebugRelocationTarget::Section(".debug".into()),
                    debug_offsets[&id] as i32,
                ),
            };
            DebugRelocation {
                offset: relocation.offset,
                kind: if aligned {
                    DebugRelocationKind::Address32
                } else if first_reference_uses_aligned_relocation {
                    DebugRelocationKind::UnalignedAddress32
                } else {
                    DebugRelocationKind::Address32
                },
                target,
                addend: relocation.addend + target_addend,
            }
        })
        .collect()
}
