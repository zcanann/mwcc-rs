//! Exact-match whole-function captures: real TUs compiled as one claimed
//! shape each (capture -> tools/dis2rust.py -> AST-hash + context gates ->
//! verbatim emission). See docs/emission-model.md and the per-file docs.

mod eacos;
mod eacos_bl;
mod easin;
mod easin_bl;
mod efmod;
mod epow;
mod erempio2;
mod ktan;
mod satan;
mod sldexp;

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::Function;

/// The total-verification gate: the Debug hash of the parsed function. Any
/// deviation in names, constants, or statement order changes it — a capture
/// template only fires on the EXACT AST it was measured against.
pub(crate) fn ast_hash(function: &Function) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{function:?}").hash(&mut hasher);
    hasher.finish()
}

/// A fingerprint of the TU's skipped-inline population — the compilation
/// CONTEXT the AST hash cannot see (header-dependent @N pool bases, inline
/// availability). Templates dispatch measured per-context values on it.
pub(crate) fn skipped_context_fingerprint(names: &std::collections::HashSet<String>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut sorted: Vec<&str> = names.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sorted.hash(&mut hasher);
    hasher.finish()
}

impl Generator {
    /// Try every capture in registration order; the AST-hash + context gates
    /// make the order irrelevant for correctness.
    pub(crate) fn try_captures(&mut self, function: &Function) -> Compilation<bool> {
        Ok(self.try_efmod(function)?
            || self.try_satan(function)?
            || self.try_ktan(function)?
            || self.try_easin(function)?
            || self.try_easin_bl(function)?
            || self.try_eacos(function)?
            || self.try_eacos_bl(function)?
            || self.try_epow(function)?
            || self.try_sldexp(function)?
            || self.try_erempio2(function)?)
    }
}
