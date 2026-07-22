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
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;

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
    if line_fragments.len() != unit.functions.len()
        || debug_fragments.len() != unit.functions.len()
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
            DebugRelocationTarget::Section(name)
                if name == ".debug" && relocation.offset == 8 =>
            {
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
                    relocation.addend -= i32::try_from(function_layout.offsets[index]).map_err(
                        |_| Diagnostic::error("debug-info: GC 4.1 function offset is too large"),
                    )?;
                }
            }
            _ => {}
        }
    }

    Ok(sections)
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
    if second_null_size < 4
        || read_u32(bytes, second_null_offset)? != 4
        || !padded_tail_is_valid
    {
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
