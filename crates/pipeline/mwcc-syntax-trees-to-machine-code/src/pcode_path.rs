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
    config: &CompilerConfig,
) -> Compilation<MachineFunction> {
    let behavior = Behavior::resolve(config);
    let small_data = behavior.global_addressing == GlobalAddressing::SmallData;
    let global_info: HashMap<String, mwcc_syntax_trees_to_pcode::GlobalInfo> = globals
        .iter()
        .map(|global| {
            (
                global.name.clone(),
                mwcc_syntax_trees_to_pcode::GlobalInfo {
                    ty: global.declared_type,
                    small_data,
                },
            )
        })
        .collect();
    let context = mwcc_syntax_trees_to_pcode::LoweringContext {
        globals: &global_info,
        call_return_types,
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
