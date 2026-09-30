//! The hook that routes a function through the staged PCode pipeline
//! (`docs/backend-pipeline-proposal.md`) when `MWCC_PCODE` is set:
//! `MWCC_PCODE=1` tries it first and falls back to the legacy owners;
//! `MWCC_PCODE=only` reports its diagnostic instead of falling back.
//!
//! The pipeline itself lives in `mwcc-pcode-path`, which the driver installs
//! with [`install_pcode_lowering`]. Keeping it out of this crate's dependency
//! graph means PCode changes never rebuild the legacy owners.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use mwcc_core::Compilation;
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};
use mwcc_versions::CompilerConfig;

/// Everything the PCode pipeline needs about one function and its unit.
pub struct PcodeRequest<'a> {
    pub function: &'a Function,
    pub globals: &'a [GlobalDeclaration],
    pub call_return_types: &'a HashMap<String, Type>,
    pub variadic_callees: &'a HashSet<String>,
    pub prototyped: &'a HashSet<String>,
    pub call_parameter_types: &'a HashMap<String, Vec<Type>>,
    pub config: &'a CompilerConfig,
    /// Whether a call is to a compiler intrinsic (`name`, argument count).
    pub is_intrinsic: &'a dyn Fn(&str, usize) -> bool,
    /// Whether this unit has a body for the named function (a call MWCC
    /// may inline).
    pub has_body: &'a dyn Fn(&str) -> bool,
}

/// A PCode lowering entry point.
pub type PcodeLowering = fn(&PcodeRequest<'_>) -> Compilation<MachineFunction>;

static LOWERING: OnceLock<PcodeLowering> = OnceLock::new();

/// Install the PCode pipeline (once, before compiling).
pub fn install_pcode_lowering(lowering: PcodeLowering) {
    let _ = LOWERING.set(lowering);
}

pub(crate) enum Mode {
    TryFirst,
    Only,
}

pub(crate) fn mode() -> Option<Mode> {
    LOWERING.get()?;
    match std::env::var("MWCC_PCODE").ok()?.as_str() {
        "" | "0" => None,
        "only" => Some(Mode::Only),
        _ => Some(Mode::TryFirst),
    }
}

pub(crate) fn lower(request: &PcodeRequest<'_>) -> Compilation<MachineFunction> {
    let lowering = LOWERING.get().expect("mode() checked the installation");
    lowering(request)
}
