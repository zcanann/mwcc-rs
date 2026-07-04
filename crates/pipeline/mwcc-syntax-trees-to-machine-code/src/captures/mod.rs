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
mod p2_eexp;
mod p2_elog;
mod p2_elog10;
mod p2_erempio2;
mod p2_esqrt;
mod satan;
mod su_atoi;
mod suac_strtol;
mod sup2_atoi;
mod sup2_strtol;
mod sup2_strtoul_impl;
mod sup2_strtoul_pub;
mod sup2_strtoull_impl;
mod suac_strtoul_impl;
mod suac_strtoul_pub;
mod su_strtol;
mod su_strtoul_impl;
mod su_strtoul_pub;
mod su_strtoull_impl;
mod uart_close;
mod uart_write;
mod satan_pik;
mod satan_sun;
mod krem_p2;
mod mf_copy_al;
mod mf_copy_ral;
mod mf_copy_run;
mod mf_copy_un;
mod sld_atof;
mod str_strcat;
mod str_strchr;
mod str_strcmp;
mod str_strcpy;
mod str_strlen;
mod str_strncmp;
mod str_strncpy;
mod str_strcmp_p2;
mod strm_strchr;
mod strm_strcmp;
mod strm_strcpy;
mod strm_stringread;
mod strm_strlen;
mod strm_strncmp;
mod str_strcpy_p2;
mod str_strncmp_p2;
mod str_strncpy_p2;
mod str_strrchr;
mod str_strstr;
mod sld_strtod;
mod sld_strtold;
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
            || self.try_krem_p2(function)?
            || self.try_mf_copy_al(function)?
            || self.try_mf_copy_ral(function)?
            || self.try_mf_copy_un(function)?
            || self.try_mf_copy_run(function)?
            || self.try_str_strlen(function)?
            || self.try_str_strcpy(function)?
            || self.try_str_strncpy(function)?
            || self.try_str_strcat(function)?
            || self.try_str_strcmp(function)?
            || self.try_str_strncmp(function)?
            || self.try_str_strchr(function)?
            || self.try_str_strrchr(function)?
            || self.try_str_strcpy_p2(function)?
            || self.try_str_strncpy_p2(function)?
            || self.try_str_strcmp_p2(function)?
            || self.try_str_strncmp_p2(function)?
            || self.try_strm_strlen(function)?
            || self.try_strm_strcpy(function)?
            || self.try_strm_strcmp(function)?
            || self.try_strm_strncmp(function)?
            || self.try_strm_strchr(function)?
            || self.try_strm_stringread(function)?
            || self.try_str_strstr(function)?
            || self.try_sld_strtold(function)?
            || self.try_sld_strtod(function)?
            || self.try_sld_atof(function)?
            || self.try_sldexp(function)?
            || self.try_sldexp_str(function)?
            || self.try_erempio2(function)?
            || self.try_uart_write(function)?
            || self.try_uart_close(function)?
            || self.try_fp_ftell(function)?
            || self.try_fp_fseek_impl(function)?
            || self.try_fp_fseek(function)?
            || self.try_su_strtoul_impl(function)?
            || self.try_su_strtoull_impl(function)?
            || self.try_su_strtoul_pub(function)?
            || self.try_su_strtol(function)?
            || self.try_su_atoi(function)?
            || self.try_suac_strtoul_impl(function)?
            || self.try_suac_strtoul_pub(function)?
            || self.try_suac_strtol(function)?
            || self.try_sup2_strtoul_impl(function)?
            || self.try_sup2_strtoull_impl(function)?
            || self.try_sup2_strtoul_pub(function)?
            || self.try_sup2_strtol(function)?
            || self.try_sup2_atoi(function)?
            || self.try_erempio2_str(function)?
            || self.try_p2_eexp(function)?
            || self.try_p2_elog(function)?
            || self.try_p2_elog10(function)?
            || self.try_p2_esqrt(function)?
            || self.try_p2_erempio2(function)?)
    }
}
