//! Fragment symbols and relocation identities used by the 4.x DWARF-1 object
//! format.
//!
//! The DWARF record bytes remain ordinary CodeWarrior DWARF 1. What changed in
//! this generation is their ELF identity: line-table components and DIE records
//! receive named object symbols, and references bind through those fragments.
//! Keep that container policy separate from semantic DIE construction so later
//! function/type families can reuse the same fragmentation owner.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{
    layout_function_placements, DebugLayout, DebugRelocationKind, DebugRelocationTarget,
    DebugSection, DebugSections, DebugSymbol, DebugSymbolBinding, DebugSymbolPlacement,
    FunctionPlacement,
};
use mwcc_syntax_trees::{TranslationUnit, Type};
use mwcc_versions::CompilerBuild;

use super::legacy::data::{fragmented_plan, FragmentedDataItem};

pub(super) fn lower_simple_void_functions(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    code_alignment: u32,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let post_framed_bump = fragmented_post_framed_bump(build);
    let (first_ordinal, line_end_ordinal) =
        fragment_ordinals(machine_functions, build, post_framed_bump)?;
    let line_header = format!(".line..{first_ordinal}");
    let line_end_name = format!(".line..{line_end_ordinal}");
    let compile_unit = format!(".dwarf.0011..{}", line_end_ordinal + 1);
    let first_null_name = format!(".dwarf.0000..{}", line_end_ordinal + 2);
    let second_null_name = format!(".dwarf.0000..{}", line_end_ordinal + 3);

    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let (debug_fragments, first_null_offset, second_null_offset, second_null_size) =
        debug_fragment_boundaries(unit, &sections.debug, compile_unit_size)?;
    let placements = machine_functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.encode_text().len() as u32,
            deferred: function.text_deferred,
        })
        .collect::<Vec<_>>();
    let function_layout = layout_function_placements(&placements, code_alignment);
    let line_fragments = line_fragment_boundaries(&sections.line, &function_layout)?;
    if line_fragments.len() != unit.functions.len() || debug_fragments.len() != unit.functions.len()
    {
        return Err(Diagnostic::error(
            "debug-info: GC 4.1 fragment count does not match emitted functions",
        ));
    }
    let line_end_offset = sections.line.len().saturating_sub(10) as u32;
    if second_null_offset + second_null_size != sections.debug.len() as u32
        || sections.line.len() < 28
        || sections.line[0..4] != (sections.line.len() as u32).to_be_bytes()
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 simple-function fragment boundaries",
        ));
    }

    let closing_placement = DebugSymbolPlacement::AfterFunctionLocals(unit.functions.len() - 1);
    sections.layout = DebugLayout::AfterDataGrouped;
    sections.post_framed_function_anonymous_bump_override = Some(post_framed_bump);
    sections.symbols = vec![
        symbol(
            line_header.clone(),
            DebugSection::Line,
            0,
            8,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            line_end_name,
            DebugSection::Line,
            line_end_offset,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            first_null_name,
            DebugSection::Debug,
            first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            second_null_name,
            DebugSection::Debug,
            second_null_offset,
            second_null_size,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
    ];
    for ((function, line), debug) in unit
        .functions
        .iter()
        .zip(&line_fragments)
        .zip(&debug_fragments)
    {
        let binding = function_binding(function.is_static, function.is_weak);
        let fragment_flags = if function.is_weak { 0x0e00_0000 } else { 0 };
        sections.symbols.push(symbol(
            format!(".line.{}", function.name),
            DebugSection::Line,
            line.offset,
            line.size,
            1,
            binding,
            fragment_flags,
            DebugSymbolPlacement::Early,
        ));
        sections.symbols.push(symbol(
            debug.name.clone(),
            DebugSection::Debug,
            debug.offset,
            debug.size,
            1,
            binding,
            fragment_flags,
            DebugSymbolPlacement::Early,
        ));
    }

    for relocation in &mut sections.line_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
    }
    for relocation in &mut sections.debug_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
        match &relocation.target {
            DebugRelocationTarget::Section(name) if name == ".debug" && relocation.offset == 8 => {
                relocation.target = DebugRelocationTarget::Symbol(compile_unit.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".line" => {
                relocation.target = DebugRelocationTarget::Symbol(line_header.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".debug" => {
                if let Some(fragment) = debug_fragments
                    .iter()
                    .find(|fragment| fragment.contains(relocation.offset))
                {
                    relocation.target = DebugRelocationTarget::Symbol(fragment.name.clone());
                    relocation.addend -= fragment.offset as i32;
                }
            }
            DebugRelocationTarget::Section(name) if name == ".text" => {
                if let Some((index, _)) = debug_fragments
                    .iter()
                    .enumerate()
                    .find(|(_, fragment)| fragment.contains(relocation.offset))
                {
                    relocation.target =
                        DebugRelocationTarget::Symbol(unit.functions[index].name.clone());
                    relocation.addend -=
                        i32::try_from(function_layout.offsets[index]).map_err(|_| {
                            Diagnostic::error("debug-info: GC 4.1 function offset is too large")
                        })?;
                }
            }
            _ => {}
        }
    }

    Ok(sections)
}

pub(super) fn lower_functions_with_aggregate_data(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    code_alignment: u32,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let post_framed_bump = fragmented_post_framed_bump(build);
    let (first_ordinal, line_end_ordinal) =
        fragment_ordinals(machine_functions, build, post_framed_bump)?;
    let line_header = format!(".line..{first_ordinal}");
    let compile_unit = format!(".dwarf.0011..{}", line_end_ordinal + 1);
    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let (function_fragments, data_start) =
        function_fragment_boundaries(unit, &sections.debug, compile_unit_size)?;
    let (data_fragments, first_null_offset, second_null_offset, second_null_size) =
        data_fragment_boundaries(unit, &sections.debug, data_start)?;
    let placements = machine_functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.encode_text().len() as u32,
            deferred: function.text_deferred,
        })
        .collect::<Vec<_>>();
    let function_layout = layout_function_placements(&placements, code_alignment);
    let line_fragments = line_fragment_boundaries(&sections.line, &function_layout)?;
    if function_fragments.len() != unit.functions.len()
        || line_fragments.len() != unit.functions.len()
        || second_null_offset + second_null_size != sections.debug.len() as u32
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 mixed fragment boundaries",
        ));
    }

    let line_end_offset = sections.line.len().saturating_sub(10) as u32;
    let closing_placement = DebugSymbolPlacement::AfterFunctionLocals(unit.functions.len() - 1);
    sections.layout = DebugLayout::AfterDataGrouped;
    sections.post_framed_function_anonymous_bump_override = Some(post_framed_bump);
    sections.symbols = vec![
        symbol(
            line_header.clone(),
            DebugSection::Line,
            0,
            8,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            format!(".line..{line_end_ordinal}"),
            DebugSection::Line,
            line_end_offset,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            format!(".dwarf.0000..{}", line_end_ordinal + 2),
            DebugSection::Debug,
            first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
        symbol(
            format!(".dwarf.0000..{}", line_end_ordinal + 3),
            DebugSection::Debug,
            second_null_offset,
            second_null_size,
            1,
            DebugSymbolBinding::Local,
            0,
            closing_placement,
        ),
    ];
    for ((function, line), debug) in unit
        .functions
        .iter()
        .zip(&line_fragments)
        .zip(&function_fragments)
    {
        let binding = function_binding(function.is_static, function.is_weak);
        let flags = if function.is_weak { 0x0e00_0000 } else { 0 };
        sections.symbols.push(symbol(
            format!(".line.{}", function.name),
            DebugSection::Line,
            line.offset,
            line.size,
            1,
            binding,
            flags,
            DebugSymbolPlacement::Early,
        ));
        sections.symbols.push(symbol(
            debug.name.clone(),
            DebugSection::Debug,
            debug.offset,
            debug.size,
            1,
            binding,
            flags,
            DebugSymbolPlacement::Early,
        ));
    }
    for fragment in &data_fragments {
        sections.symbols.push(symbol(
            fragment.name.clone(),
            DebugSection::Debug,
            fragment.offset,
            fragment.size,
            1,
            fragment.binding,
            fragment.comment_flags,
            DebugSymbolPlacement::Early,
        ));
    }

    for relocation in &mut sections.line_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
    }
    for relocation in &mut sections.debug_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
        match &relocation.target {
            DebugRelocationTarget::Section(name) if name == ".debug" && relocation.offset == 8 => {
                relocation.target = DebugRelocationTarget::Symbol(compile_unit.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".line" => {
                relocation.target = DebugRelocationTarget::Symbol(line_header.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".debug" => {
                if let Some(fragment) = function_fragments
                    .iter()
                    .find(|fragment| fragment.contains(relocation.offset))
                {
                    relocation.target = DebugRelocationTarget::Symbol(fragment.name.clone());
                    relocation.addend -= fragment.offset as i32;
                } else if let Some(fragment) = reference_fragment(
                    &data_fragments,
                    relocation.offset,
                    u32::try_from(relocation.addend).unwrap_or(u32::MAX),
                ) {
                    relocation.target = DebugRelocationTarget::Symbol(fragment.name.clone());
                    relocation.addend -= fragment.offset as i32;
                }
            }
            DebugRelocationTarget::Section(name) if name == ".text" => {
                if let Some((index, _)) = function_fragments
                    .iter()
                    .enumerate()
                    .find(|(_, fragment)| fragment.contains(relocation.offset))
                {
                    relocation.target =
                        DebugRelocationTarget::Symbol(unit.functions[index].name.clone());
                    relocation.addend -=
                        i32::try_from(function_layout.offsets[index]).map_err(|_| {
                            Diagnostic::error("debug-info: GC 4.1 function offset is too large")
                        })?;
                }
            }
            _ => {}
        }
    }
    Ok(sections)
}

/// Partition one polymorphic C++ class unit into GC 4.1's class, callable,
/// vtable, RTTI, and modified-type ELF identities.
pub(super) fn lower_class_unit(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    code_alignment: u32,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let class = unit.cxx_abi_classes.first().ok_or_else(|| {
        Diagnostic::error("debug-info: GC 4.1 class fragment has no class identity")
    })?;
    let member_count = unit
        .aggregate_definitions
        .get(&class.source_name)
        .ok_or_else(|| Diagnostic::error("debug-info: GC 4.1 class layout was not retained"))?
        .members
        .len();
    let vtable = format!("__vt__{}", class.encoded_name);
    let rtti = format!("__RTTI__{}", class.encoded_name);
    let post_framed_bump = fragmented_post_framed_bump(build);
    let (first_ordinal, line_end_ordinal) =
        class_fragment_ordinals(machine_functions, build, post_framed_bump)?;
    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let boundaries = class_fragment_boundaries(
        &sections.debug,
        compile_unit_size,
        member_count,
        &class.source_name,
        &unit.functions,
        &vtable,
        &rtti,
        line_end_ordinal,
        function_comment_flags(unit, &unit.functions[1]),
    )?;
    let placements = machine_functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.encode_text().len() as u32,
            deferred: function.text_deferred,
        })
        .collect::<Vec<_>>();
    let function_layout = layout_function_placements(&placements, code_alignment);
    let line_fragments = line_fragment_boundaries(&sections.line, &function_layout)?;
    if line_fragments.len() != unit.functions.len()
        || boundaries.second_null_offset + boundaries.second_null_size
            != sections.debug.len() as u32
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 class fragment boundaries",
        ));
    }

    let line_header = format!(".line..{first_ordinal}");
    let compile_unit = format!(".dwarf.0011..{}", line_end_ordinal + 1);
    let closing = DebugSymbolPlacement::AfterFunctionLocals(unit.functions.len() - 1);
    sections.layout = DebugLayout::AfterDataGrouped;
    sections.post_framed_function_anonymous_bump_override = Some(post_framed_bump);
    sections.symbols = vec![
        symbol(
            line_header.clone(),
            DebugSection::Line,
            0,
            8,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            format!(".line..{line_end_ordinal}"),
            DebugSection::Line,
            sections.line.len().saturating_sub(10) as u32,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
            closing,
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
            closing,
        ),
    ];
    for fragment in boundaries
        .fragments
        .iter()
        .filter(|fragment| fragment.binding == DebugSymbolBinding::Local)
    {
        sections.symbols.push(symbol(
            fragment.name.clone(),
            DebugSection::Debug,
            fragment.offset,
            fragment.size,
            1,
            fragment.binding,
            fragment.comment_flags,
            closing,
        ));
    }
    sections.symbols.extend([
        symbol(
            format!(".dwarf.0000..{}", line_end_ordinal + 8),
            DebugSection::Debug,
            boundaries.first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            closing,
        ),
        symbol(
            format!(".dwarf.0000..{}", line_end_ordinal + 9),
            DebugSection::Debug,
            boundaries.second_null_offset,
            boundaries.second_null_size,
            1,
            DebugSymbolBinding::Local,
            0,
            closing,
        ),
    ]);
    for (function, line) in unit.functions.iter().zip(&line_fragments) {
        sections.symbols.push(symbol(
            format!(".line.{}", function.name),
            DebugSection::Line,
            line.offset,
            line.size,
            1,
            function_binding(function.is_static, function.is_weak),
            function_comment_flags(unit, function),
            DebugSymbolPlacement::Early,
        ));
    }
    for fragment in boundaries
        .fragments
        .iter()
        .filter(|fragment| fragment.binding != DebugSymbolBinding::Local)
    {
        sections.symbols.push(symbol(
            fragment.name.clone(),
            DebugSection::Debug,
            fragment.offset,
            fragment.size,
            1,
            fragment.binding,
            fragment.comment_flags,
            DebugSymbolPlacement::Early,
        ));
    }

    for relocation in &mut sections.line_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
    }
    for relocation in &mut sections.debug_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
        match &relocation.target {
            DebugRelocationTarget::Section(name) if name == ".debug" && relocation.offset == 8 => {
                relocation.target = DebugRelocationTarget::Symbol(compile_unit.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".line" => {
                relocation.target = DebugRelocationTarget::Symbol(line_header.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".debug" => {
                let target_offset = u32::try_from(relocation.addend).unwrap_or(u32::MAX);
                // A DW_AT_sibling may point exactly at the next fragment while
                // remaining relative to its source fragment. All other exact
                // boundary references name the target DIE's fragment. The
                // attribute code immediately before the relocation field keeps
                // this distinction semantic instead of offset-specific.
                let is_sibling = relocation
                    .offset
                    .checked_sub(2)
                    .and_then(|start| {
                        sections
                            .debug
                            .get(start as usize..relocation.offset as usize)
                    })
                    == Some(&[0x00, 0x12][..]);
                let exact_target = (!is_sibling).then(|| {
                    boundaries
                        .fragments
                        .iter()
                        .find(|fragment| fragment.offset == target_offset)
                });
                if let Some(fragment) = exact_target.flatten().or_else(|| {
                    reference_fragment(
                        &boundaries.fragments,
                        relocation.offset,
                        target_offset,
                    )
                }) {
                    relocation.target = DebugRelocationTarget::Symbol(fragment.name.clone());
                    relocation.addend -= fragment.offset as i32;
                }
            }
            DebugRelocationTarget::Section(name) if name == ".text" => {
                if let Some((index, _fragment)) = boundaries
                    .fragments
                    .iter()
                    .filter(|fragment| fragment.name.starts_with(".dwarf.0006."))
                    .enumerate()
                    .find(|(_, fragment)| fragment.contains(relocation.offset))
                {
                    relocation.target =
                        DebugRelocationTarget::Symbol(unit.functions[index].name.clone());
                    relocation.addend -= function_layout.offsets[index] as i32;
                }
            }
            _ => {}
        }
    }
    Ok(sections)
}

struct ClassFragmentBoundaries {
    fragments: Vec<DataFragmentBoundary>,
    first_null_offset: u32,
    second_null_offset: u32,
    second_null_size: u32,
}

#[allow(clippy::too_many_arguments)]
fn class_fragment_boundaries(
    bytes: &[u8],
    compile_unit_size: u32,
    member_count: usize,
    class_name: &str,
    functions: &[mwcc_syntax_trees::Function],
    vtable: &str,
    rtti: &str,
    line_end_ordinal: u32,
    destructor_comment_flags: u32,
) -> Compilation<ClassFragmentBoundaries> {
    let mut cursor = compile_unit_size;
    let mut fragments = Vec::new();
    let mut take = |name: String,
                    records: usize,
                    has_null: bool,
                    binding: DebugSymbolBinding,
                    comment_flags: u32|
     -> Compilation<()> {
        let offset = cursor;
        for _ in 0..records {
            cursor = advance_record(bytes, cursor)?;
        }
        if has_null {
            if read_u32(bytes, cursor)? != 4 {
                return Err(Diagnostic::error(
                    "debug-info: invalid GC 4.1 class child terminator",
                ));
            }
            cursor += 4;
        }
        fragments.push(DataFragmentBoundary {
            name,
            offset,
            size: cursor - offset,
            binding,
            comment_flags,
        });
        Ok(())
    };

    for (delta, records, has_null) in [(2, 2, true), (3, 2, true), (4, 1, false), (5, 2, true)] {
        take(
            format!(".dwarf.0015..{}", line_end_ordinal + delta),
            records,
            has_null,
            DebugSymbolBinding::Local,
            0,
        )?;
    }
    let terminal = class_name.rsplit("::").next().unwrap_or(class_name);
    take(
        format!(".dwarf.0002.{class_name}::{terminal}"),
        member_count + 2,
        true,
        DebugSymbolBinding::Weak,
        0x0d00_0000,
    )?;
    take(
        format!(".dwarf.0006.{}", functions[0].name),
        1,
        false,
        function_binding(functions[0].is_static, functions[0].is_weak),
        0,
    )?;
    take(
        format!(".dwarf.0013..{}", line_end_ordinal + 6),
        1,
        false,
        DebugSymbolBinding::Local,
        0,
    )?;
    take(
        format!(".dwarf.0007.{vtable}"),
        1,
        false,
        DebugSymbolBinding::Weak,
        0x0d00_0000,
    )?;
    take(
        format!(".dwarf.0013..{}", line_end_ordinal + 7),
        1,
        false,
        DebugSymbolBinding::Local,
        0,
    )?;
    take(
        format!(".dwarf.0007.{rtti}"),
        1,
        false,
        DebugSymbolBinding::Weak,
        0x0d00_0000,
    )?;
    take(
        format!(".dwarf.0006.{}", functions[1].name),
        2,
        true,
        function_binding(functions[1].is_static, functions[1].is_weak),
        destructor_comment_flags,
    )?;
    let first_null_offset = cursor;
    let second_null_offset = first_null_offset + 4;
    let second_null_size = bytes.len() as u32 - second_null_offset;
    if read_u32(bytes, first_null_offset)? != 4
        || read_u32(bytes, second_null_offset)? != 4
        || second_null_size < 4
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 class unit terminators",
        ));
    }
    Ok(ClassFragmentBoundaries {
        fragments,
        first_null_offset,
        second_null_offset,
        second_null_size,
    })
}

/// Give a functionless aggregate-data unit the fragment symbols used by the
/// GC 4.1 object container. The legacy lowering above this pass remains the
/// owner of DWARF semantics and bytes; this pass only partitions that record
/// stream and redirects its relocations through the resulting ELF symbols.
pub(super) fn lower_aggregate_data_unit(
    unit: &TranslationUnit,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let (fragments, first_null_offset, second_null_offset, second_null_size) =
        data_fragment_boundaries(unit, &sections.debug, compile_unit_size)?;
    if sections.line.len() != 18
        || sections.line[0..4] != (sections.line.len() as u32).to_be_bytes()
        || first_null_offset + 4 != second_null_offset
        || second_null_offset + second_null_size != sections.debug.len() as u32
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 aggregate-data fragment boundaries",
        ));
    }

    let line_header = ".line..1".to_string();
    let compile_unit = ".dwarf.0011..3".to_string();
    let callable_fragments = fragments
        .iter()
        .filter(|fragment| fragment.name.starts_with(".dwarf.0015.."))
        .count() as u32;
    let first_null_ordinal = 4 + callable_fragments;
    sections.layout = DebugLayout::AfterDataGrouped;
    sections.symbols = vec![
        symbol(
            line_header.clone(),
            DebugSection::Line,
            0,
            8,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            ".line..2".into(),
            DebugSection::Line,
            8,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
    ];
    for fragment in fragments
        .iter()
        .filter(|fragment| fragment.binding == DebugSymbolBinding::Local)
    {
        sections.symbols.push(symbol(
            fragment.name.clone(),
            DebugSection::Debug,
            fragment.offset,
            fragment.size,
            1,
            fragment.binding,
            fragment.comment_flags,
            DebugSymbolPlacement::Early,
        ));
    }
    sections.symbols.extend([
        symbol(
            format!(".dwarf.0000..{first_null_ordinal}"),
            DebugSection::Debug,
            first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            format!(".dwarf.0000..{}", first_null_ordinal + 1),
            DebugSection::Debug,
            second_null_offset,
            second_null_size,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::Early,
        ),
    ]);
    for fragment in fragments
        .iter()
        .filter(|fragment| fragment.binding != DebugSymbolBinding::Local)
    {
        sections.symbols.push(symbol(
            fragment.name.clone(),
            DebugSection::Debug,
            fragment.offset,
            fragment.size,
            1,
            fragment.binding,
            fragment.comment_flags,
            DebugSymbolPlacement::Early,
        ));
    }

    for relocation in &mut sections.line_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
    }
    for relocation in &mut sections.debug_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
        match &relocation.target {
            DebugRelocationTarget::Section(name) if name == ".debug" && relocation.offset == 8 => {
                relocation.target = DebugRelocationTarget::Symbol(compile_unit.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".line" => {
                relocation.target = DebugRelocationTarget::Symbol(line_header.clone());
            }
            DebugRelocationTarget::Section(name) if name == ".debug" => {
                let target_offset = u32::try_from(relocation.addend).unwrap_or(u32::MAX);
                let is_sibling = relocation
                    .offset
                    .checked_sub(2)
                    .and_then(|start| {
                        sections
                            .debug
                            .get(start as usize..relocation.offset as usize)
                    }) == Some(&[0x00, 0x12][..]);
                let exact_target = (!is_sibling).then(|| {
                    fragments
                        .iter()
                        .find(|fragment| fragment.offset == target_offset)
                });
                if let Some(fragment) = exact_target.flatten().or_else(|| {
                    reference_fragment(
                        &fragments,
                        relocation.offset,
                        target_offset,
                    )
                }) {
                    relocation.target = DebugRelocationTarget::Symbol(fragment.name.clone());
                    relocation.addend -= fragment.offset as i32;
                }
            }
            _ => {}
        }
    }

    Ok(sections)
}

pub(super) fn matches_aggregate_data_unit(unit: &TranslationUnit) -> bool {
    let globals = emitted_debug_globals(unit);
    !globals.is_empty()
        && globals
            .iter()
            .all(|global| matches!(global.declared_type, Type::Struct { .. }))
}

#[derive(Clone, Debug)]
struct DataFragmentBoundary {
    name: String,
    offset: u32,
    size: u32,
    binding: DebugSymbolBinding,
    comment_flags: u32,
}

impl DataFragmentBoundary {
    fn contains(&self, offset: u32) -> bool {
        offset >= self.offset && offset < self.offset + self.size
    }

    fn contains_including_end(&self, offset: u32) -> bool {
        offset >= self.offset && offset <= self.offset + self.size
    }
}

fn data_fragment_boundaries(
    unit: &TranslationUnit,
    bytes: &[u8],
    start_offset: u32,
) -> Compilation<(Vec<DataFragmentBoundary>, u32, u32, u32)> {
    let mut cursor = start_offset;
    let mut fragments = Vec::new();
    let mut callable_ordinal = 4u32;
    for item in fragmented_plan(unit)? {
        match item {
            FragmentedDataItem::Callable { function_type } => {
                if !unit.functions.is_empty() {
                    return Err(Diagnostic::error(
                        "debug-info: callable data fragments alongside functions are not implemented yet (roadmap)",
                    ));
                }
                let return_offset = cursor;
                cursor = advance_record(bytes, cursor)?;
                fragments.push(DataFragmentBoundary {
                    name: format!(".dwarf.0015..{callable_ordinal}"),
                    offset: return_offset,
                    size: cursor - return_offset,
                    binding: DebugSymbolBinding::Local,
                    comment_flags: 0,
                });
                callable_ordinal += 1;

                let callable_offset = cursor;
                cursor = advance_record(bytes, cursor)?;
                // A source `void(void)` callable has one DWARF formal-parameter
                // sentinel followed by its child-list terminator.
                if function_type.variadic || !function_type.parameters.is_empty() {
                    return Err(Diagnostic::error(
                        "debug-info: this callable data fragment is not implemented yet (roadmap)",
                    ));
                }
                cursor = advance_record(bytes, cursor)?;
                if read_u32(bytes, cursor)? != 4 {
                    return Err(Diagnostic::error(
                        "debug-info: invalid callable child terminator",
                    ));
                }
                cursor += 4;
                fragments.push(DataFragmentBoundary {
                    name: format!(".dwarf.0015..{callable_ordinal}"),
                    offset: callable_offset,
                    size: cursor - callable_offset,
                    binding: DebugSymbolBinding::Local,
                    comment_flags: 0,
                });
                callable_ordinal += 1;
            }
            FragmentedDataItem::Aggregate { definition, .. } => {
                let offset = cursor;
                cursor = advance_record(bytes, cursor)?;
                for _ in &definition.members {
                    cursor = advance_record(bytes, cursor)?;
                }
                if read_u32(bytes, cursor)? != 4 {
                    return Err(Diagnostic::error(
                        "debug-info: invalid GC 4.1 aggregate-children terminator",
                    ));
                }
                cursor += 4;
                let tag = if definition.is_union { "0017" } else { "0013" };
                let ordinary_name = definition.source_tag.as_deref().unwrap_or(&definition.name);
                let qualified_callable_name = definition
                    .members
                    .iter()
                    .any(|member| member.function_type.is_some())
                    .then(|| format!("{ordinary_name}::{ordinary_name}"));
                let name = qualified_callable_name.as_deref().unwrap_or(ordinary_name);
                fragments.push(DataFragmentBoundary {
                    name: format!(".dwarf.{tag}.{name}"),
                    offset,
                    size: cursor - offset,
                    binding: DebugSymbolBinding::Weak,
                    comment_flags: 0x0d00_0000,
                });
            }
            FragmentedDataItem::Global { global, .. } => {
                let offset = cursor;
                cursor = advance_record(bytes, cursor)?;
                fragments.push(DataFragmentBoundary {
                    name: format!(".dwarf.0007.{}", global.name),
                    offset,
                    size: cursor - offset,
                    binding: if global.is_weak {
                        DebugSymbolBinding::Weak
                    } else {
                        DebugSymbolBinding::Global
                    },
                    comment_flags: if global.is_weak { 0x0d00_0000 } else { 0 },
                });
            }
        }
    }

    let first_null_offset = cursor;
    if read_u32(bytes, first_null_offset)? != 4 {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 data-list terminator",
        ));
    }
    let second_null_offset = first_null_offset + 4;
    let second_null_size = u32::try_from(bytes.len())
        .ok()
        .and_then(|length| length.checked_sub(second_null_offset))
        .filter(|size| *size >= 4)
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 unit terminator"))?;
    if read_u32(bytes, second_null_offset)? != 4 {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 unit terminator",
        ));
    }
    Ok((
        fragments,
        first_null_offset,
        second_null_offset,
        second_null_size,
    ))
}

fn emitted_debug_globals(unit: &TranslationUnit) -> Vec<&mwcc_syntax_trees::GlobalDeclaration> {
    unit.globals
        .iter()
        .filter(|global| !global.is_extern && !global.is_static && !global.name.is_empty())
        .collect()
}

fn reference_fragment<'a>(
    fragments: &'a [DataFragmentBoundary],
    relocation_offset: u32,
    target_offset: u32,
) -> Option<&'a DataFragmentBoundary> {
    // A sibling reference to the byte immediately after a fragment still
    // binds through that source fragment. Prefer this relationship before an
    // exact-start match with the following fragment.
    if let Some(source) = fragments
        .iter()
        .find(|fragment| fragment.contains(relocation_offset))
    {
        if source.contains_including_end(target_offset) {
            return Some(source);
        }
    }
    fragments
        .iter()
        .find(|fragment| fragment.offset == target_offset)
        .or_else(|| {
            fragments
                .iter()
                .find(|fragment| fragment.contains(target_offset))
        })
}

#[derive(Clone, Debug)]
struct FragmentBoundary {
    name: String,
    offset: u32,
    size: u32,
}

impl FragmentBoundary {
    fn contains(&self, offset: u32) -> bool {
        offset >= self.offset && offset < self.offset + self.size
    }
}

#[derive(Clone, Copy, Debug)]
struct LineBoundary {
    offset: u32,
    size: u32,
}

fn debug_fragment_boundaries(
    unit: &TranslationUnit,
    bytes: &[u8],
    compile_unit_size: u32,
) -> Compilation<(Vec<FragmentBoundary>, u32, u32, u32)> {
    let (fragments, cursor) = function_fragment_boundaries(unit, bytes, compile_unit_size)?;
    let first_null_offset = cursor;
    if read_u32(bytes, first_null_offset)? != 4 {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 function-list terminator",
        ));
    }
    let second_null_offset = first_null_offset + 4;
    let second_null_size = u32::try_from(bytes.len())
        .ok()
        .and_then(|length| length.checked_sub(second_null_offset))
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 unit terminator"))?;
    let padded_tail_is_valid = if second_null_size == 4 {
        true
    } else if second_null_size > 4 {
        read_u32(bytes, second_null_offset + 4)? == second_null_size - 4
    } else {
        false
    };
    if second_null_size < 4 || read_u32(bytes, second_null_offset)? != 4 || !padded_tail_is_valid {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 padded unit terminator",
        ));
    }
    Ok((
        fragments,
        first_null_offset,
        second_null_offset,
        second_null_size,
    ))
}

fn function_fragment_boundaries(
    unit: &TranslationUnit,
    bytes: &[u8],
    compile_unit_size: u32,
) -> Compilation<(Vec<FragmentBoundary>, u32)> {
    let mut cursor = compile_unit_size;
    let mut fragments = Vec::with_capacity(unit.functions.len());
    for function in &unit.functions {
        let offset = cursor;
        cursor = advance_record(bytes, cursor)?;
        if !function.statements.is_empty() {
            cursor = advance_record(bytes, cursor)?;
            if read_u32(bytes, cursor)? != 4 {
                return Err(Diagnostic::error(
                    "debug-info: invalid GC 4.1 parameter-list terminator",
                ));
            }
            cursor += 4;
        }
        fragments.push(FragmentBoundary {
            name: format!(".dwarf.0006.{}", function.name),
            offset,
            size: cursor - offset,
        });
    }
    Ok((fragments, cursor))
}

fn advance_record(bytes: &[u8], offset: u32) -> Compilation<u32> {
    let size = read_u32(bytes, offset)?;
    if size < 4 {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 DWARF record size",
        ));
    }
    offset
        .checked_add(size)
        .filter(|end| *end <= bytes.len() as u32)
        .ok_or_else(|| Diagnostic::error("debug-info: truncated GC 4.1 DWARF record"))
}

fn line_fragment_boundaries(
    bytes: &[u8],
    layout: &mwcc_object::FunctionLayout,
) -> Compilation<Vec<LineBoundary>> {
    if bytes.len() < 18 || (bytes.len() - 8) % 10 != 0 {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 line-table record stream",
        ));
    }
    let line_end_offset = bytes.len() as u32 - 10;
    let mut cursor = 8u32;
    let mut fragments = Vec::with_capacity(layout.offsets.len());
    for index in 0..layout.offsets.len() {
        let start = cursor;
        let function_start = layout.offsets[index];
        let function_end = function_start + layout.sizes[index];
        while cursor < line_end_offset {
            let address = read_u32(bytes, cursor + 6)?;
            if address < function_start || address >= function_end {
                break;
            }
            cursor += 10;
        }
        if cursor == start {
            return Err(Diagnostic::error(
                "debug-info: GC 4.1 function has no line fragment",
            ));
        }
        fragments.push(LineBoundary {
            offset: start,
            size: cursor - start,
        });
    }
    if cursor != line_end_offset {
        return Err(Diagnostic::error(
            "debug-info: GC 4.1 line records do not partition by function",
        ));
    }
    Ok(fragments)
}

fn fragment_ordinals(
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    post_framed_bump: u8,
) -> Compilation<(u32, u32)> {
    let mut counter = u32::from(build.initial_anonymous_counter);
    let first_prefix = ordinal_bump_before_unwind(&machine_functions[0])?;
    let first_ordinal = counter
        .checked_add(first_prefix.saturating_sub(1))
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?;
    let mut close_ordinal = None;
    for (index, machine) in machine_functions.iter().enumerate() {
        let mut number = counter
            .checked_add(ordinal_bump_before_unwind(machine)?)
            .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?;
        if machine.frame.is_some() {
            number = number
                .checked_add(2)
                .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?;
        }
        if index + 1 == machine_functions.len() {
            close_ordinal = Some(
                number
                    .checked_add(u32::from(machine.frame.is_none()))
                    .ok_or_else(|| {
                        Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal")
                    })?,
            );
        }
        let post_function_bump = machine.post_function_anonymous_bump.unwrap_or_else(|| {
            if machine.frame.is_some() {
                post_framed_bump
            } else {
                build.post_leaf_function_anonymous_bump
            }
        });
        counter = number
            .checked_add(u32::from(post_function_bump))
            .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?;
    }
    Ok((
        first_ordinal,
        close_ordinal.expect("a simple-function debug unit is nonempty"),
    ))
}

fn class_fragment_ordinals(
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    post_framed_bump: u8,
) -> Compilation<(u32, u32)> {
    if build.version != (4, 1, 0) {
        return Err(Diagnostic::error(
            "debug-info: fragmented class ordinals are only measured for GC 4.1",
        ));
    }
    let (ordinary_first, _ordinary_end) =
        fragment_ordinals(machine_functions, build, post_framed_bump)?;
    let first = ordinary_first
        .checked_add(2)
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 class ordinal"))?;
    let end = first
        .checked_add(7)
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 class ordinal"))?;
    Ok((first, end))
}

fn fragmented_post_framed_bump(build: CompilerBuild) -> u8 {
    if build.version == (4, 1, 0) {
        // With `-sym on`, GC 4.1 moves one of the ordinary four framed
        // post-function ordinals into the function's preceding analysis block.
        // Two consecutive framed functions expose a three-ordinal transition.
        3
    } else {
        build.post_framed_function_anonymous_bump
    }
}

fn function_binding(is_static: bool, is_weak: bool) -> DebugSymbolBinding {
    if is_weak {
        DebugSymbolBinding::Weak
    } else if is_static {
        DebugSymbolBinding::Local
    } else {
        DebugSymbolBinding::Global
    }
}

fn function_comment_flags(
    unit: &TranslationUnit,
    function: &mwcc_syntax_trees::Function,
) -> u32 {
    if !function.is_weak {
        0
    } else if unit
        .weak_materialized
        .iter()
        .any(|name| name == &function.name)
    {
        0x0d00_0000
    } else {
        0x0e00_0000
    }
}

/// Return the anonymous work preceding a pool-free function's unwind records.
///
/// The simple-void family owns no strings, constants, read-only blobs, or jump
/// tables. Its fragmented line header is therefore the last ordinal before the
/// unwind pair: instruction-selection work plus the generation-specific hidden
/// labels charged after constants. Keep that relationship explicit here until
/// the fragmented router admits functions with object payloads, which require a
/// full translation-unit ordinal plan rather than another local sum.
fn ordinal_bump_before_unwind(machine: &MachineFunction) -> Compilation<u32> {
    if !machine.string_literals.is_empty()
        || !machine.constants.is_empty()
        || !machine.anonymous_rodata.is_empty()
        || !machine.jump_tables.is_empty()
        || !machine.static_locals.is_empty()
    {
        return Err(Diagnostic::error(
            "debug-info: GC 4.1 simple-function fragment unexpectedly owns anonymous payload",
        ));
    }
    machine
        .object_anonymous_bump()
        .checked_add(machine.fragmented_debug_anonymous_bump)
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?
        .checked_add(machine.post_constant_label_bump)
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))
}

fn read_u32(bytes: &[u8], offset: u32) -> Compilation<u32> {
    let start = offset as usize;
    let value = bytes
        .get(start..start + 4)
        .ok_or_else(|| Diagnostic::error("debug-info: truncated GC 4.1 DWARF record"))?;
    Ok(u32::from_be_bytes(value.try_into().unwrap()))
}

fn symbol(
    name: String,
    section: DebugSection,
    offset: u32,
    size: u32,
    alignment: u32,
    binding: DebugSymbolBinding,
    comment_flags: u32,
    placement: DebugSymbolPlacement,
) -> DebugSymbol {
    DebugSymbol {
        name,
        section,
        offset,
        size,
        alignment,
        comment_flags,
        binding,
        placement,
    }
}
