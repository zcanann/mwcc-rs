//! Exact-match whole-function captures: real TUs compiled as one claimed
//! shape each (capture -> tools/dis2rust.py -> AST-hash + context gates ->
//! verbatim emission). See docs/emission-model.md and the per-file docs.

mod eacos;
mod fp_fseek;
mod fp_fseek_impl;
mod fp_ftell;
mod eacos_bl;
mod eacos_str;
mod easin;
mod easin_bl;
mod easin_str;
mod eatan2_ac;
mod eatan2_pik;
mod eatan2_sun;
mod eatan2_ww;
mod efmod;
mod epow;
mod epow_ww;
mod erempio2;
mod erempio2_str;
mod ktan;
mod ktan_ww;
mod satan;
mod uart_close;
mod uart_write;
mod satan_pik;
mod satan_sun;
mod sldexp;
mod sldexp_str;

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
        // MWCC_CAPTURE_DEBUG=1: print every function's identity triple so a
        // census can spot hash-matched TUs whose context isn't registered.
        if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() {
            eprintln!(
                "capture-census: {} hash={:#x} ctx={:#x}",
                function.name,
                ast_hash(function),
                skipped_context_fingerprint(&self.skipped_inline_names)
            );
        }
        Ok(self.try_efmod(function)?
            || self.try_satan(function)?
            || self.try_satan_pik(function)?
            || self.try_satan_sun(function)?
            || self.try_ktan(function)?
            || self.try_ktan_ww(function)?
            || self.try_easin(function)?
            || self.try_easin_bl(function)?
            || self.try_easin_str(function)?
            || self.try_eacos(function)?
            || self.try_eacos_bl(function)?
            || self.try_eacos_str(function)?
            || self.try_eatan2_pik(function)?
            || self.try_eatan2_ac(function)?
            || self.try_eatan2_sun(function)?
            || self.try_eatan2_ww(function)?
            || self.try_epow(function)?
            || self.try_epow_ww(function)?
            || self.try_sldexp(function)?
            || self.try_sldexp_str(function)?
            || self.try_erempio2(function)?
            || self.try_uart_write(function)?
            || self.try_uart_close(function)?
            || self.try_fp_ftell(function)?
            || self.try_fp_fseek_impl(function)?
            || self.try_fp_fseek(function)?
            || self.try_erempio2_str(function)?)
    }
}
