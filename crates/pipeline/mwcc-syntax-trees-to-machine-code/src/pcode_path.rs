//! Route a function through the staged PCode pipeline
//! (`docs/backend-pipeline-proposal.md`) when `MWCC_PCODE` is set:
//! `MWCC_PCODE=1` tries it first and falls back to the legacy owners;
//! `MWCC_PCODE=only` reports its diagnostic instead of falling back.

use std::collections::HashMap;

use mwcc_core::Compilation;
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};
use mwcc_versions::{Behavior, CompilerConfig, GlobalAddressing};

pub(crate) enum Mode {
    TryFirst,
    Only,
}

pub(crate) fn mode() -> Option<Mode> {
    match std::env::var("MWCC_PCODE").ok()?.as_str() {
        "" | "0" => None,
        "only" => Some(Mode::Only),
        _ => Some(Mode::TryFirst),
    }
}

pub(crate) fn lower(
    function: &Function,
    globals: &[GlobalDeclaration],
    call_return_types: &HashMap<String, Type>,
    variadic_callees: &std::collections::HashSet<String>,
    prototyped: &std::collections::HashSet<String>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
    config: &CompilerConfig,
) -> Compilation<MachineFunction> {
    let behavior = Behavior::resolve(config);
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
    let global_info: HashMap<String, mwcc_syntax_trees_to_pcode::GlobalInfo> = globals
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
        call_return_types,
        is_intrinsic: &crate::intrinsics::is_intrinsic_call,
        variadic_callees,
        prototyped,
        call_parameter_types,
    };
    let lowered = mwcc_syntax_trees_to_pcode::lower(function, &context)?;
    let mut output = mwcc_pcode_to_machine_code::finish(
        lowered.pcode,
        lowered.makes_calls,
        mwcc_pcode_to_machine_code::FinishOptions {
            schedule: behavior.schedule_latency_slots,
            delete_dead: behavior.optimization != mwcc_versions::Optimization::O0,
        },
    )?;
    output.section = function.section.clone();
    Ok(output)
}
