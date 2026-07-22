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
    DebugLayout, DebugRelocationKind, DebugRelocationTarget, DebugSection, DebugSections,
    DebugSymbol, DebugSymbolBinding, DebugSymbolPlacement,
};
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;

pub(super) fn lower_single_simple_void(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let function = &unit.functions[0];
    let machine = &machine_functions[0];
    let prefix_bump = ordinal_bump_before_unwind(machine)?;
    let first_ordinal = u32::from(build.initial_anonymous_counter)
        .checked_add(prefix_bump.saturating_sub(1))
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal"))?;
    let intervening_locals = if machine.frame.is_some() { 2 } else { 0 };
    let line_end_ordinal = first_ordinal + intervening_locals + 1;
    let line_header = format!(".line..{first_ordinal}");
    let line_end = format!(".line..{line_end_ordinal}");
    let compile_unit = format!(".dwarf.0011..{}", line_end_ordinal + 1);
    let first_null = format!(".dwarf.0000..{}", line_end_ordinal + 2);
    let second_null = format!(".dwarf.0000..{}", line_end_ordinal + 3);
    let function_line = format!(".line.{}", function.name);
    let function_debug = format!(".dwarf.0006.{}", function.name);

    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let function_offset = compile_unit_size;
    let function_size = sections
        .debug
        .len()
        .checked_sub(function_offset as usize + 8)
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| Diagnostic::error("debug-info: invalid GC 4.1 function fragment"))?;
    let first_null_offset = function_offset + function_size;
    let second_null_offset = first_null_offset + 4;
    let line_end_offset = sections.line.len().saturating_sub(10) as u32;
    let function_line_size = line_end_offset.saturating_sub(8);
    if second_null_offset + 4 != sections.debug.len() as u32
        || read_u32(&sections.debug, function_offset)? > function_size
        || sections.line.len() < 28
        || sections.line[0..4] != (sections.line.len() as u32).to_be_bytes()
        || function_line_size == 0
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 simple-function fragment boundaries",
        ));
    }

    let binding = if function.is_weak {
        DebugSymbolBinding::Weak
    } else if function.is_static {
        DebugSymbolBinding::Local
    } else {
        DebugSymbolBinding::Global
    };
    let fragment_flags = if function.is_weak { 0x0d00_0000 } else { 0 };
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
            line_end,
            DebugSection::Line,
            line_end_offset,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::AfterFunctionLocals(0),
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::AfterFunctionLocals(0),
        ),
        symbol(
            first_null,
            DebugSection::Debug,
            first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::AfterFunctionLocals(0),
        ),
        symbol(
            second_null,
            DebugSection::Debug,
            second_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
            DebugSymbolPlacement::AfterFunctionLocals(0),
        ),
        symbol(
            function_line,
            DebugSection::Line,
            8,
            function_line_size,
            1,
            binding,
            fragment_flags,
            DebugSymbolPlacement::Early,
        ),
        symbol(
            function_debug.clone(),
            DebugSection::Debug,
            function_offset,
            function_size,
            1,
            binding,
            fragment_flags,
            DebugSymbolPlacement::Early,
        ),
    ];

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
            DebugRelocationTarget::Section(name)
                if name == ".debug" && relocation.offset >= function_offset =>
            {
                relocation.target = DebugRelocationTarget::Symbol(function_debug.clone());
                relocation.addend -= function_offset as i32;
            }
            DebugRelocationTarget::Section(name)
                if name == ".text" && relocation.offset >= function_offset =>
            {
                relocation.target = DebugRelocationTarget::Symbol(function.name.clone());
            }
            _ => {}
        }
    }

    Ok(sections)
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
