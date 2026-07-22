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
    DebugSymbol, DebugSymbolBinding,
};
use mwcc_syntax_trees::{TranslationUnit, Type};
use mwcc_versions::CompilerBuild;

pub(super) fn matches_single_empty_leaf(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    has_emitted_data: bool,
) -> bool {
    if has_emitted_data || unit.functions.len() != 1 || machine_functions.len() != 1 {
        return false;
    }
    let function = &unit.functions[0];
    function.return_type == Type::Void
        && function.parameters.is_empty()
        && function.locals.is_empty()
        && function.statements.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
        && machine_functions[0].encode_text().len() == 4
}

pub(super) fn lower_single_empty_leaf(
    unit: &TranslationUnit,
    build: CompilerBuild,
    mut sections: DebugSections,
) -> Compilation<DebugSections> {
    let function = &unit.functions[0];
    let first_ordinal = build.initial_anonymous_counter as u32;
    let line_header = format!(".line..{first_ordinal}");
    let line_end = format!(".line..{}", first_ordinal + 1);
    let compile_unit = format!(".dwarf.0011..{}", first_ordinal + 2);
    let first_null = format!(".dwarf.0000..{}", first_ordinal + 3);
    let second_null = format!(".dwarf.0000..{}", first_ordinal + 4);
    let function_line = format!(".line.{}", function.name);
    let function_debug = format!(".dwarf.0006.{}", function.name);

    let compile_unit_size = read_u32(&sections.debug, 0)?;
    let function_offset = compile_unit_size;
    let function_size = read_u32(&sections.debug, function_offset)?;
    let first_null_offset = function_offset + function_size;
    let second_null_offset = first_null_offset + 4;
    if second_null_offset + 4 != sections.debug.len() as u32
        || sections.line.len() != 28
        || sections.line[0..4] != 28_u32.to_be_bytes()
    {
        return Err(Diagnostic::error(
            "debug-info: invalid GC 4.1 empty-leaf fragment boundaries",
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
        ),
        symbol(
            line_end,
            DebugSection::Line,
            18,
            10,
            1,
            DebugSymbolBinding::Local,
            0,
        ),
        symbol(
            compile_unit.clone(),
            DebugSection::Debug,
            0,
            compile_unit_size,
            4,
            DebugSymbolBinding::Local,
            0,
        ),
        symbol(
            first_null,
            DebugSection::Debug,
            first_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
        ),
        symbol(
            second_null,
            DebugSection::Debug,
            second_null_offset,
            4,
            1,
            DebugSymbolBinding::Local,
            0,
        ),
        symbol(
            function_line,
            DebugSection::Line,
            8,
            10,
            1,
            binding,
            fragment_flags,
        ),
        symbol(
            function_debug.clone(),
            DebugSection::Debug,
            function_offset,
            function_size,
            1,
            binding,
            fragment_flags,
        ),
    ];

    for relocation in &mut sections.line_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
    }
    for relocation in &mut sections.debug_relocations {
        relocation.kind = DebugRelocationKind::UnalignedAddress32;
        match (&relocation.target, relocation.offset) {
            (DebugRelocationTarget::Section(name), 8) if name == ".debug" => {
                relocation.target = DebugRelocationTarget::Symbol(compile_unit.clone());
            }
            (DebugRelocationTarget::Section(name), 0x52) if name == ".line" => {
                relocation.target = DebugRelocationTarget::Symbol(line_header.clone());
            }
            (DebugRelocationTarget::Section(name), 0x5e) if name == ".debug" => {
                relocation.target = DebugRelocationTarget::Symbol(function_debug.clone());
                relocation.addend -= function_offset as i32;
            }
            (DebugRelocationTarget::Section(name), 0x7c) if name == ".text" => {
                relocation.target = DebugRelocationTarget::Symbol(function.name.clone());
            }
            _ => {}
        }
    }

    Ok(sections)
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
) -> DebugSymbol {
    DebugSymbol {
        name,
        section,
        offset,
        size,
        alignment,
        comment_flags,
        binding,
    }
}
