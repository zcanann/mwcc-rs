//! Drive one function through the staged PCode pipeline
//! (`docs/backend-pipeline-proposal.md`): INITIAL CODE
//! (`mwcc-syntax-trees-to-pcode`), then scheduling, coloring, frame and final
//! code (`mwcc-pcode-to-machine-code`).
//!
//! The legacy backend calls this through the hook it exposes
//! ([`mwcc_syntax_trees_to_machine_code::install_pcode_lowering`]); the driver
//! installs [`lower`] at startup. Only this crate depends on both sides, so a
//! PCode change rebuilds the PCode crates, this one, and the driver.

use std::collections::HashMap;

use mwcc_core::Compilation;
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
    if behavior.optimization != mwcc_versions::Optimization::O4 {
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
    let small_data = behavior.global_addressing == GlobalAddressing::SmallData;
    let global_info: HashMap<String, mwcc_syntax_trees_to_pcode::GlobalInfo> = request
        .globals
        .iter()
        .map(|global| {
            (
                global.name.clone(),
                mwcc_syntax_trees_to_pcode::GlobalInfo {
                    ty: global.declared_type,
                    small_data,
                    is_array: global.array_length.is_some(),
                    is_volatile: global.is_volatile,
                },
            )
        })
        .collect();
    let context = mwcc_syntax_trees_to_pcode::LoweringContext {
        globals: &global_info,
        call_return_types: request.call_return_types,
        is_intrinsic: request.is_intrinsic,
        variadic_callees: request.variadic_callees,
        prototyped: request.prototyped,
        call_parameter_types: request.call_parameter_types,
    };
    let lowered = mwcc_syntax_trees_to_pcode::lower(request.function, &context)?;
    let mut output = mwcc_pcode_to_machine_code::finish(
        lowered.pcode,
        lowered.makes_calls,
        mwcc_pcode_to_machine_code::FinishOptions {
            schedule: behavior.schedule_latency_slots,
            delete_dead: behavior.optimization != mwcc_versions::Optimization::O0,
            two_integer_units: behavior.integer_select_style
                == mwcc_versions::IntegerSelectStyle::Branchless,
        },
    )?;
    output.section = request.function.section.clone();
    Ok(output)
}
