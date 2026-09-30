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

/// Lower `request.function`, or explain what is not modeled yet.
pub fn lower(request: &PcodeRequest<'_>) -> Compilation<MachineFunction> {
    let behavior = Behavior::resolve(request.config);
    let unoptimized = behavior.optimization == mwcc_versions::Optimization::O0;
    if behavior.optimization != mwcc_versions::Optimization::O4 && !unoptimized {
        // Lower levels skip IRO passes and keep stack-resident variables;
        // only the -O4 pipeline is modeled so far.
        return Err(mwcc_core::Diagnostic::error(
            "PCode lowering: only -O4 is modeled (not yet supported)",
        ));
    }
    if behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::BranchPreserving {
        // Selects and comparison values are modeled on the branchless builds.
        return Err(mwcc_core::Diagnostic::error(
            "PCode lowering: branch-preserving select builds (not yet supported)",
        ));
    }
    let globals: HashMap<String, GlobalInfo> = request
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
                },
            )
        })
        .collect();
    let unit = mwcc_iro::Unit {
        globals: &globals,
        call_return_types: request.call_return_types,
        is_intrinsic: request.is_intrinsic,
        variadic_callees: request.variadic_callees,
        prototyped: request.prototyped,
        call_parameter_types: request.call_parameter_types,
        has_body: request.has_body,
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
        },
    )?;
    output.section = request.function.section.clone();
    Ok(output)
}
