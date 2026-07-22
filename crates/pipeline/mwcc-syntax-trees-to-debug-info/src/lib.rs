//! Pipeline: parsed source + finalized machine functions -> CodeWarrior DWARF 1.
//!
//! The syntax tree supplies names, types, and physical source provenance; the
//! machine representation supplies final code sizes and deferred layout state.
//! DWARF byte encoding and ELF container policy remain in their own crates.

mod fragmented;
mod legacy;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{DebugEntryId, RelocationTarget};
use mwcc_machine_code::MachineFunction;
use mwcc_object::{DebugRelocation, DebugRelocationKind, DebugRelocationTarget, DebugSections};
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::CompilerBuild;
use std::collections::{HashMap, HashSet};

/// Route debug lowering by object-format generation. The legacy encoder owns
/// the grouped DWARF-1 model; fragmented generations remain an explicit seam.
pub fn lower_debug_info(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    has_emitted_data: bool,
    emitted_data_symbols: &HashSet<String>,
    source_name: &str,
    source: &[u8],
    build: CompilerBuild,
    code_alignment: u32,
) -> Compilation<Option<DebugSections>> {
    // Debug sections describe emitted definitions, not merely parsed source.
    // A typedef-only unit and a preprocessed header bridge whose inline bodies
    // were all dropped both produce the same comment-only object as a lexically
    // empty file. Conversely, a functionless data unit still needs DWARF. Keep
    // this decision at the boundary where executable and data lowering have
    // already established what survives into the object.
    if machine_functions.is_empty() && !has_emitted_data {
        return Ok(None);
    }
    // Exact captures are generation-independent semantic debug payloads. Give
    // them first refusal before routing uncaptured units to a format-specific
    // synthesizer; the object writer still owns section/symbol/relocation layout.
    if let Some(capture) =
        legacy::lookup_capture(unit, machine_functions, source_name, source, build)?
    {
        return Ok(Some(capture));
    }
    let fragmented_generation = build.version.0 >= 4;
    // Functionless data units retain the same monolithic DWARF-1 DIE stream in
    // the later generations. Their container layout moved after ordinary data,
    // which the legacy data lowering already models independently. Fragmented
    // `.dwarf.*` symbols first appear in the 4.x generation. GC/1.3.2 build 81
    // instead keeps the monolithic grouped stream used by the legacy lowering.
    let monolithic_data_unit = unit.functions.is_empty() && machine_functions.is_empty();
    if fragmented_generation && monolithic_data_unit {
        if fragmented::matches_aggregate_data_unit(unit) {
            let grouped = legacy::lower(
                unit,
                machine_functions,
                emitted_data_symbols,
                source_name,
                build,
                code_alignment,
            )?;
            return fragmented::lower_aggregate_data_unit(unit, grouped).map(Some);
        }
    } else if fragmented_generation {
        if !has_emitted_data {
            let grouped = legacy::lower(
                unit,
                machine_functions,
                emitted_data_symbols,
                source_name,
                build,
                code_alignment,
            )?;
            return fragmented::lower_simple_void_functions(
                unit,
                machine_functions,
                build,
                code_alignment,
                grouped,
            )
            .map(Some);
        }
        if fragmented::matches_aggregate_data_unit(unit) {
            let grouped = legacy::lower(
                unit,
                machine_functions,
                emitted_data_symbols,
                source_name,
                build,
                code_alignment,
            )?;
            return fragmented::lower_functions_with_aggregate_data(
                unit,
                machine_functions,
                build,
                code_alignment,
                grouped,
            )
            .map(Some);
        }
        return Err(Diagnostic::error(
            "debug-info: this compiler generation's fragmented/interleaved object format is not implemented yet (roadmap)",
        ));
    }

    legacy::lower(
        unit,
        machine_functions,
        emitted_data_symbols,
        source_name,
        build,
        code_alignment,
    )
    .map(Some)
}

fn convert_relocations(
    relocations: Vec<mwcc_dwarf1::Relocation>,
    debug_offsets: &HashMap<DebugEntryId, u32>,
    unaligned_addresses: bool,
) -> Vec<DebugRelocation> {
    relocations
        .into_iter()
        .map(|relocation| {
            let (target, target_addend) = match relocation.target {
                RelocationTarget::External(name) if name.starts_with('.') => {
                    (DebugRelocationTarget::Section(name), 0)
                }
                RelocationTarget::External(name) => (DebugRelocationTarget::Symbol(name), 0),
                RelocationTarget::DebugEntry(id) => (
                    DebugRelocationTarget::Section(".debug".into()),
                    debug_offsets[&id] as i32,
                ),
            };
            DebugRelocation {
                offset: relocation.offset,
                // MWCC uses UADDR32 only when the four-byte field itself is
                // unaligned. Dynamic DIE sizes can make later addresses aligned.
                kind: if unaligned_addresses && relocation.offset % 4 != 0 {
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
