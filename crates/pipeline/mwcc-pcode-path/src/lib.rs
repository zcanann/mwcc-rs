//! Drive one function through the staged PCode pipeline
//! (`docs/backend-pipeline-proposal.md`):
//!
//! 1. `mwcc-syntax-trees-to-iro`: build the typed IRO representation and run
//!    the IRO passes (folding, algebra, idioms, selects, stores);
//! 2. `mwcc-iro-to-pcode`: INITIAL CODE (instruction selection);
//! 3. `mwcc-pcode-to-machine-code`: scheduling, coloring, frame, final code.
//!
//! The legacy backend calls this through the hook it exposes
//! ([`mwcc_syntax_trees_to_machine_code::install_pcode_lowering`]); the driver
//! installs [`lower`] at startup. Only this crate depends on both sides, so a
//! PCode change rebuilds the PCode crates, this one, and the driver.

use std::collections::HashMap;

use mwcc_core::Compilation;
use mwcc_iro::GlobalInfo;
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees_to_machine_code::PcodeRequest;
use mwcc_versions::{Behavior, GlobalAddressing};

/// Install the pipeline as the legacy backend's PCode hook.
pub fn install() {
    mwcc_syntax_trees_to_machine_code::install_pcode_lowering(lower);
}

/// Whether a definition has nonzero contents (`.data`; zero is `.bss`).
fn initialized(bytes: Option<&[u8]>, values: Option<&[i64]>, relocated: bool) -> bool {
    relocated || bytes.is_some_and(|bytes| bytes.iter().any(|&byte| byte != 0)) || values.is_some_and(|values| values.iter().any(|&value| value != 0))
}

/// Lower `request.function`, or explain what is not modeled yet.
pub fn lower(request: &PcodeRequest<'_>) -> Compilation<MachineFunction> {
    // An internal error in one function is reported as that function's
    // diagnostic instead of ending the whole unit.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lower_function(request))) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("panic");
            Err(mwcc_core::Diagnostic::error(format!("PCode lowering: internal error ({message})")))
        }
    }
}

fn lower_function(request: &PcodeRequest<'_>) -> Compilation<MachineFunction> {
    if std::env::var_os("MWCC_PCODE_TRACE").is_some() {
        eprintln!("mwcc: lowering '{}'", request.function.name);
    }
    let behavior = Behavior::resolve(request.config);
    let unoptimized = behavior.optimization == mwcc_versions::Optimization::O0;
    // -O2/-O3 take the -O4 pipeline (unscheduled: MWCC schedules by
    // default only at -O4); their own transformations are not modeled.
    if behavior.optimization == mwcc_versions::Optimization::O1 {
        return Err(mwcc_core::Diagnostic::error("PCode lowering: -O1 is not modeled (not yet supported)"));
    }
    let early_frame = matches!(request.config.build.label, "GC/1.0" | "GC/1.1" | "GC/1.2.5" | "GC/1.2.5n");
    // (GC/1.1p1 shares the early frame shapes without the reservation.)
    let patch_frame = request.config.build.label == "GC/1.1p1";
    // (GC/1.1p1 keeps a frame of its own, not modeled.)
    if behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::BranchPreserving && !early_frame && !patch_frame {
        return Err(mwcc_core::Diagnostic::error(
            "PCode lowering: the GC/1.1p1 frame (not yet supported)",
        ));
    }
    let no_inline_bodies = HashMap::new();
    // Objects this unit defines (a tentative or initialized definition).
    let defined: std::collections::HashSet<&str> = request
        .globals
        .iter()
        .filter(|global| !global.is_extern && std::env::var_os("MWCC_PCODE_NO_SECTION_ANCHORS").is_none())
        .map(|global| global.name.as_str())
        .collect();
    let mut globals: HashMap<String, GlobalInfo> = request
        .globals
        .iter()
        .map(|global| {
            // Objects of at most 8 bytes live in small data (when enabled).
            let addressing =
                if global.is_const { behavior.read_only_global_addressing } else { behavior.global_addressing };
            let element = match global.declared_type {
                mwcc_syntax_trees::Type::Struct { size, .. } => Some(size),
                other => Some(u32::from(other.width()) / 8).filter(|&size| size > 0),
            };
            let size = match (global.array_length, global.array_length_inferred) {
                (Some(length), false) => element.map(|element| element * u32::from(length)),
                (None, false) => element,
                _ => None,
            };
            let small_data = addressing == GlobalAddressing::SmallData && size.is_some_and(|size| size <= 8);
            (
                global.name.clone(),
                GlobalInfo {
                    ty: global.declared_type,
                    small_data,
                    is_array: global.array_length.is_some() || global.array_length_inferred,
                    is_volatile: global.is_volatile,
                    is_function: false,
                    is_const: global.is_const && !global.is_volatile,
                    anchor: (defined.contains(global.name.as_str()) && !small_data && !global.is_const)
                        .then(|| if initialized(global.data_bytes.as_deref(), global.initializer.as_deref(), !global.data_relocations.is_empty() || global.address_initializer.is_some()) { "...data.0" } else { "...bss.0" }),
                    fixed_address: None,
                    // (The driver drops a `static const` scalar's object.)
                    folded: (global.is_static
                        && global.is_const
                        && !global.is_volatile
                        && global.array_length.is_none()
                        && global.address_initializer.is_none()
                        && global.data_bytes.is_none())
                    .then(|| global.initializer.as_ref().and_then(|values| values.first().copied()).unwrap_or(0)),
                },
            )
        })
        .collect();
    // A section is anchored only while every object of it sits within a
    // signed 16-bit displacement of its start.
    for section in ["...bss.0", "...data.0"] {
        let total: u32 = request
            .globals
            .iter()
            .filter(|global| globals.get(&global.name).is_some_and(|info| info.anchor == Some(section)))
            .map(|global| {
                let element = match global.declared_type {
                    mwcc_syntax_trees::Type::Struct { size, .. } => size,
                    other => u32::from(other.width()) / 8,
                };
                // (An unknown size counts as out of range.)
                match (global.array_length, global.array_length_inferred) {
                    (_, true) => 0x8000,
                    (Some(length), false) => element * u32::from(length),
                    (None, false) => element.max(1),
                }
                .div_ceil(8)
                    * 8
            })
            .sum();
        if total > 0x7fff {
            for info in globals.values_mut() {
                if info.anchor == Some(section) {
                    info.anchor = None;
                }
            }
        }
    }
    // Function statics address like globals of their size.
    for local in request.static_locals {
        let addressing = if local.is_const { behavior.read_only_global_addressing } else { behavior.global_addressing };
        let element = match local.declared_type {
            mwcc_syntax_trees::Type::Struct { size, .. } => size,
            other => u32::from(other.width()) / 8,
        };
        let size = element * local.array_length.map_or(1, u32::from);
        globals.insert(
            local.name.clone(),
            GlobalInfo {
                ty: local.declared_type,
                small_data: addressing == GlobalAddressing::SmallData && size > 0 && size <= 8,
                is_array: local.array_length.is_some(),
                is_volatile: local.is_volatile,
                is_function: false,
                is_const: local.is_const && !local.is_volatile,
                anchor: None,
                fixed_address: None,
                    folded: None,
            },
        );
    }
    for (name, &(address, element)) in request.fixed_address_arrays {
        globals.entry(name.clone()).or_insert(GlobalInfo {
            ty: element,
            small_data: false,
            is_array: true,
            is_volatile: true,
            is_function: false,
            is_const: false,
            anchor: None,
            fixed_address: Some(address),
            folded: None,
        });
    }
    // A function named as a value is its (absolute) address.
    for name in request.call_return_types.keys() {
        if !globals.contains_key(name) && std::env::var_os("MWCC_PCODE_NO_FUNCTION_ADDRESS").is_none() {
            globals.insert(
                name.clone(),
                GlobalInfo {
                    ty: mwcc_syntax_trees::Type::Int,
                    small_data: false,
                    is_array: true,
                    is_volatile: false,
                    is_function: true,
                    is_const: false,
                    anchor: None,
                    fixed_address: None,
                    folded: None,
                },
            );
        }
    }
    let unit = mwcc_iro::Unit {
        globals: &globals,
        call_return_types: request.call_return_types,
        pointer_return_type: request.pointer_return_type,
        is_intrinsic: request.is_intrinsic,
        variadic_callees: request.variadic_callees,
        prototyped: request.prototyped,
        call_parameter_types: request.call_parameter_types,
        has_body: request.has_body,
        inline_bodies: if std::env::var_os("MWCC_PCODE_NO_O0_INLINE").is_some() && unoptimized {
            &no_inline_bodies
        } else {
            request.inline_bodies
        },
        unoptimized,
        shift_add_multiply: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        equality_subtracts_constant: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        tied_halves: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        early_frame,
        branch_preserving: early_frame || patch_frame,
        reassociates_sums: !request.config.build.label.starts_with("GC/3.") && !request.config.build.label.starts_with("Wii/"),
        // (GC/3.x divides by multiplication at every level.)
        // (And at any level with an explicit `,p`.)
        magic_division: behavior.optimization == mwcc_versions::Optimization::O4
            || request.config.flags.explicit_speed_goal
            || request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        signed_promoted_truth: request.config.build.label.starts_with("GC/3.") || request.config.build.label.starts_with("Wii/"),
        nonvolatile_pointers: request.nonvolatile_pointers,
        keeps_struct_stores: request.config.build.label.starts_with("GC/3.") || request.config.build.label.starts_with("Wii/"),
        const_globals_across_calls: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        strings_small_data: behavior.global_addressing == GlobalAddressing::SmallData,
        strings_packed: behavior.string_literals_packed,
        returns_bool: request.returns_bool,
        cxx: request.cxx,
        bit_field_declared_units: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        pool_small_data: behavior.read_only_global_addressing == GlobalAddressing::SmallData,
        narrow_parameters_extended: request.config.build.label.starts_with("GC/3.")
            || request.config.build.label.starts_with("Wii/"),
        switch_style: if request.config.build.label.starts_with("Wii/") {
            2
        } else if request.config.build.label.starts_with("GC/3.") {
            1
        } else {
            0
        },
    };
    let built = if unoptimized {
        mwcc_syntax_trees_to_iro::build_unoptimized_compile(request.function, &unit)?
    } else {
        mwcc_syntax_trees_to_iro::build(request.function, &unit)?
    };
    if std::env::var("MWCC_IRO_DUMP").is_ok_and(|name| name == built.function.name || name == "1") {
        eprint!("{}", built.function.listing());
    }
    let lowered = mwcc_iro_to_pcode::lower(
        &built.function,
        built.returns_through_variable,
        &unit,
        unoptimized,
        behavior.tail_call_optimization,
        behavior.contract_floating_point,
    )?;
    let mut output = mwcc_pcode_to_machine_code::finish(
        lowered.pcode,
        lowered.makes_calls,
        mwcc_pcode_to_machine_code::FinishOptions {
            schedule: behavior.schedule_latency_slots,
            delete_dead: behavior.optimization != mwcc_versions::Optimization::O0,
            two_integer_units: behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::Branchless,
            unoptimized,
            fold_absolute_into_own_base: request.config.build.label.starts_with("GC/3.")
                || request.config.build.label.starts_with("Wii/"),
            link_reload_after_float_restores: behavior.saved_float_epilogue_style
                == mwcc_versions::SavedFloatEpilogueStyle::LinkReloadAfterFloatRestores
                || request.config.build.label.starts_with("GC/3.")
                || request.config.build.label.starts_with("Wii/"),
            general_save_helper_minimum: behavior.general_save_helper_minimum,
            use_lmw_stmw: request.config.flags.use_lmw_stmw,
            early_frame: early_frame || patch_frame,
            link_reload_after_pop: patch_frame,
        },
    )?;
    output.section = request.function.section.clone();
    stamp_object_metadata(request, &behavior, &mut output);
    Ok(output)
}

/// The per-function object facts the writer reads: linkage flags, the
/// order referenced symbols are created, implicitly declared callees, and
/// the unwind frame summary.
fn stamp_object_metadata(
    request: &PcodeRequest<'_>,
    behavior: &mwcc_versions::Behavior,
    output: &mut mwcc_machine_code::MachineFunction,
) {
    use mwcc_machine_code::{FrameInfo, Instruction, RelocationKind, RelocationTarget};
    let function = request.function;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.text_deferred = function.text_deferred;
    output.force_active = function.force_active;
    if function.name.contains("@unnamed@") && !output.static_locals.is_empty() {
        output.static_locals_lead = true;
    }
    // Referenced names in relocation order (GC 3/Wii create symbols as their
    // relocations are emitted).
    let mut seen = std::collections::HashSet::new();
    let relocation_order: Vec<String> = output
        .relocations
        .iter()
        .filter_map(|relocation| match &relocation.target {
            RelocationTarget::External(name) | RelocationTarget::ExternalWithAddend(name, _) => Some(name.clone()),
            _ => None,
        })
        .filter(|name| !name.starts_with("@@") && seen.insert(name.clone()))
        .collect();
    if behavior.symbol_traversal_style == mwcc_versions::SymbolTraversalStyle::RelocationOrder {
        output.symbol_order = relocation_order.clone();
    }
    output.referenced_function_symbols = relocation_order
        .iter()
        .filter(|name| request.call_return_types.contains_key(name.as_str()))
        .cloned()
        .collect();
    // A call target without a prototype was declared implicitly at the call.
    if behavior.symbol_traversal_style != mwcc_versions::SymbolTraversalStyle::RelocationOrder {
        let mut seen = std::collections::HashSet::new();
        for relocation in &output.relocations {
            if let (RelocationKind::Rel24, RelocationTarget::External(name)) = (&relocation.kind, &relocation.target) {
                let helper = name.starts_with("_savegpr_") || name.starts_with("_restgpr_");
                if !helper && !request.prototyped.contains(name.as_str()) && seen.insert(name.clone()) {
                    output.implicit_external_callees.push(name.clone());
                }
            }
        }
    }
    // Unwind tables: a function with a frame, with C++ exceptions on.
    let framed = output
        .instructions
        .iter()
        .any(|instruction| matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }));
    let calls = output.relocations.iter().any(|relocation| {
        relocation.kind == RelocationKind::Rel24
            && matches!(&relocation.target, RelocationTarget::External(name)
                if !name.starts_with("_savegpr_") && !name.starts_with("_restgpr_"))
    });
    if framed && request.config.flags.cpp_exceptions && (calls || behavior.emit_leaf_frame_unwind) {
        let mut general = std::collections::BTreeSet::new();
        let mut float = std::collections::BTreeSet::new();
        for (index, instruction) in output.instructions.iter().enumerate() {
            match instruction {
                Instruction::StoreWord { s, a: 1, .. } if *s >= 14 => {
                    general.insert(*s);
                }
                Instruction::StoreFloatDouble { s, a: 1, .. } if *s >= 14 => {
                    float.insert(*s);
                }
                Instruction::BranchAndLink { .. } => {
                    let target = output.relocations.iter().find(|relocation| relocation.instruction_index == index);
                    if let Some(RelocationTarget::External(name)) = target.map(|relocation| &relocation.target) {
                        if let Some(first) = name.strip_prefix("_savegpr_").and_then(|n| n.parse::<u32>().ok()) {
                            general.extend(first..32);
                        }
                    }
                }
                _ => {}
            }
        }
        let touches_fpu = output.instructions.iter().any(|instruction| instruction.is_single_precision_floating_point());
        let single_arithmetic = output.instructions.iter().any(|instruction| instruction.is_single_precision_arithmetic());
        output.frame = Some(FrameInfo {
            saved_gpr_count: general.len() as u8,
            saved_fpr_count: float.len() as u8,
            uses_fpu: behavior.mark_single_precision_extab && ((calls && touches_fpu) || single_arithmetic),
        });
    }
}
