//! Exact-match whole-function captures: real TUs compiled as one claimed
//! shape each (capture -> tools/dis2rust.py -> AST-hash + context gates ->
//! verbatim emission). See docs/emission-model.md and the per-file docs.

mod abex_abort;
mod abex_exit;
mod abexb_abort;
mod abexb_exit;
mod abexm_exit;
mod abexp2_exit;
mod abexpik_exit;
mod abexs_exit;
mod acf_ctz;
mod acf_ctzl;
mod acf_dec2num;
mod acf_dorounddecup;
mod acf_must_round;
mod acf_num2dec;
mod acf_num2dec_i;
mod acf_rounddec;
mod acf_str2dec;
mod acf_timesdec;
mod acf_two_exp;
mod acf_ull2dec;
mod afp_ctzl;
mod afp_dec2num_pik;
mod afp_num2dec;
mod afp_num2dec_i;
mod afp_num2dec_mel;
mod afp_num2dec_pik;
mod afp_str2dec;
mod afp_timesdec;
mod afp_two_exp;
mod afp_ull2dec;
mod alm_block_link;
mod alm_dealloc_fixed;
mod alm_dealloc_var;
mod alm_free;
mod alm_get_malloc_pool;
mod alm_merge_next;
mod alm_merge_prev;
mod alm_pool_free;
mod alm_unlink;
mod alp_free;
mod als_alloc_var;
mod als_block_construct;
mod als_block_subblock;
mod als_link_new_block;
mod als_malloc;
mod als_pool_alloc;
mod als_soft_alloc;
mod alw_block_link;
mod ansif_close_all;
mod ansif_close_all_cr;
mod ansif_find_unopened;
mod ansif_flush_all;
mod ansif_flush_all_str;
mod ansif_flush_line;
mod ansif_init_file;
mod ar_checksize;
mod ari_abs;
mod ari_div;
mod ari_div_ww;
mod ari_labs;
mod bfp_ctz;
mod bfp_ctzl;
mod bfp_dec2num;
mod bfp_dorounddecup;
mod bfp_equals_dec;
mod bfp_less_dec;
mod bfp_minus_dec;
mod bfp_must_round;
mod bfp_num2dec;
mod bfp_num2dec_i;
mod bfp_rounddec;
mod bfp_str2dec;
mod bfp_timesdec;
mod bfp_two_exp;
mod bfp_ull2dec;
mod bio_conv_from;
mod bio_conv_to;
mod bio_flush_a;
mod bio_flush_b;
mod bio_flush_pik;
mod bio_flush_str;
mod bio_load_str;
mod bio_prep_a;
mod bio_prep_b;
mod bio_setbuf_stub;
mod bio_setvbuf_str;
mod bio_setvbuf_stub;
mod bsr_bsearch;
mod cbk_alloc;
mod cbk_erasecb;
mod cbk_free;
mod cbk_getfat;
mod cbk_updatefat;
mod cbk_writecb;
mod cck_check;
mod cck_checkasync;
mod cck_checkex;
mod cck_checkexasync;
mod cck_checksum;
mod cck_verify;
mod cck_verifydir;
mod cck_verifyfat;
mod cck_verifyid;
mod cdd_erase;
mod cdd_getdir;
mod cdd_update;
mod cdd_write;
mod cdl_async;
mod cdl_delete;
mod cdl_deletecb;
mod cdl_fast;
mod cdl_fastasync;
mod cio_fgets;
mod cio_fputs;
mod cio_put_char;
mod cop_access;
mod cop_close;
mod cop_cmpname;
mod cop_fastopen;
mod cop_getfileno;
mod cop_isopened;
mod cop_ispublic;
mod cop_isreadable;
mod cop_iswritable;
mod cop_open;
mod crd_cancel;
mod crd_read;
mod crd_readasync;
mod crd_readcb;
mod crd_seek;
mod crt_async;
mod crt_callback;
mod crt_create;
mod crw_bytes;
mod crw_read;
mod crw_readcb;
mod crw_write;
mod crw_writecb;
mod cst_getstatus;
mod cst_setasync;
mod cst_setstatus;
mod cst_updateicon;
mod cwr_erasecb;
mod cwr_write;
mod cwr_writeasync;
mod cwr_writecb;
mod dio_fread_impl_str;
mod dio_fread_impl_stub;
mod dio_fread_str;
mod dio_fread_stub;
mod dio_fwrite_full;
mod dio_fwrite_impl;
mod dio_fwrite_mid;
mod dio_fwrite_tiny;
mod dq_check;
mod dq_clear;
mod dq_dequeue;
mod dq_isin;
mod dq_pop;
mod dq_popprio;
mod dq_push;
mod dvd_convert;
mod dvd_error2num;
mod dvd_storeerr;
mod eac_acos;
mod eacos;
mod eacos_bl;
mod eacos_str;
mod easin;
mod easin_bl;
mod easin_str;
mod eat_atan2;
mod eatan2_ac;
mod eatan2_pik;
mod eatan2_sun;
mod eatan2_ww;
mod efmod;
mod epow;
mod epow_ww;
mod epw_pow;
mod epw_scalbn;
mod epx_pow;
mod erempio2;
mod erempio2_str;
mod erp_rempio2;
mod esq_sqrt;
mod exp_bfbb;
mod fio_fclose;
mod fio_fflush;
mod fio_fflush_cr;
mod fio_strnicmp_leaf;
mod fio_strnicmp_sv;
mod fp_fseek;
mod fp_fseek_impl;
mod fp_ftell;
mod fps_fseek;
mod fps_fseek_i;
mod fps_ftell;
mod fps_ftell_i;
mod fpt_fseek;
mod fpt_fseek_i;
mod fpt_ftell;
mod fpt_ftell_i;
mod fpw_fseek_i;
mod fpw_ftell;
mod fpw_ftell_i;
mod gcn_sys_alloc;
mod gcn_sys_free;
mod gcnp_sys_free;
mod gcnw_sys_free;
mod gdc_destroy;
mod gdc_register;
mod gek_register;
mod gek_unregister;
mod ivt_acosf;
mod ivt_atan2f;
mod ivt_atan_ff;
mod ivt_atanf;
mod ivt_inv_sqrtf;
mod krem_p2;
mod ktan;
mod ktan_ww;
mod ktn_tan;
mod ldx_ldexp;
mod mbs_is_utf8;
mod mbs_mblen_stub;
mod mbs_mbrlen_stub;
mod mbs_mbrtowc_stub;
mod mbs_mbsrtowcs_stub;
mod mbs_mbstowcs_str;
mod mbs_mbstowcs_stub;
mod mbs_mbtowc_bfbb;
mod mbs_mbtowc_pik;
mod mbs_mbtowc_str;
mod mbs_mbtowc_ww;
mod mbs_unicode;
mod mem_copy_policy;
mod mbs_unicode_ac;
mod mbs_utf8_bfbb;
mod mbs_utf8_ww;
mod mbs_wcrtomb_stub;
mod mbs_wcsrtombs_stub;
mod mbs_wcstombs_ac;
mod mbs_wcstombs_bfbb;
mod mbs_wcstombs_direct;
mod mbs_wcstombs_mp4;
mod mbs_wcstombs_pik;
mod mbs_wcstombs_str;
mod mbs_wctomb_ac;
mod mbs_wctomb_stub;
mod mem_fill;
mod mem_memchr;
mod mem_memchr_mel;
mod mem_memchr_mp4;
mod mem_memcmp;
mod mem_memcmp_mel;
mod mem_memcmp_mp4;
mod mem_memcpy;
mod mem_memmove;
mod mem_memmove_mp4;
mod mem_memrchr;
mod mem_memrchr_mp4;
mod mem_memset;
mod mf_copy_al;
mod mf_copy_al_mel;
mod mf_copy_al_mp4;
mod mf_copy_ral;
mod mf_copy_ral_mp4;
mod mf_copy_run;
mod mf_copy_run_mp4;
mod mf_copy_un;
mod mf_copy_un_mp4;
mod mfp_ctz;
mod mfp_ctzl;
mod mfp_dec2num;
mod mfp_dorounddecup;
mod mfp_equals_dec;
mod mfp_less_dec;
mod mfp_minus_dec;
mod mfp_must_round;
mod mfp_num2dec;
mod mfp_num2dec_i;
mod mfp_rounddec;
mod mfp_str2dec;
mod mfp_timesdec;
mod mfp_two_exp;
mod mfp_ull2dec;
mod mpc_atanf;
mod mpc_cosf;
mod mpc_fpclassifyd;
mod mpc_fpclassifyf;
mod mpc_powf;
mod mpc_sinf;
mod mtc_logf;
mod mth_fabsf;
mod mth_frexp;
mod oal_callback;
mod oal_cancel;
mod oal_init;
mod oal_insert;
mod oal_set;
mod oal_setperiodic;
mod oal_settimer;
mod ose_count;
mod ose_init;
mod ose_signal;
mod ose_trywait;
mod ose_wait;
mod osm_initqueue;
mod osm_jam;
mod osm_receive;
mod osm_send;
mod osy_initsc;
mod osy_resetsw;
mod p2_eexp;
mod p2_elog;
mod p2_elog10;
mod p2_erempio2;
mod p2_esqrt;
mod pfa_double2hex;
mod pfa_filewrite;
mod pfa_float2str;
mod pfa_long2str;
mod pfa_longlong2str;
mod pfa_parse_format;
mod pfa_pformatter;
mod pfa_printf;
mod pfa_round_decimal;
mod pfa_snprintf;
mod pfa_sprintf;
mod pfa_stringwrite;
mod pfa_vprintf;
mod pfa_vsnprintf;
mod pfa_vsprintf;
mod pfb_filewrite;
mod pfb_float2str;
mod pfb_fprintf;
mod pfb_long2str;
mod pfb_longlong2str;
mod pfb_parse_format;
mod pfb_pformatter;
mod pfb_printf;
mod pfb_round_decimal;
mod pfb_snprintf;
mod pfb_snprintf_pik;
mod pfb_sprintf;
mod pfb_stringwrite;
mod pfb_vprintf;
mod pfb_vsnprintf;
mod pfb_vsprintf;
mod pfc_printf;
mod pfc_vprintf;
mod pfd_fprintf;
mod pfd_printf;
mod pfp_ctz;
mod pfp_ctzl;
mod pfp_dec2num;
mod pfp_dorounddecup;
mod pfp_equals_dec;
mod pfp_less_dec;
mod pfp_minus_dec;
mod pfp_must_round;
mod pfp_num2dec;
mod pfp_num2dec_i;
mod pfp_rounddec;
mod pfp_str2dec;
mod pfp_timesdec;
mod pfp_two_exp;
mod pfp_ull2dec;
mod qst_qsort;
mod rt_va_arg;
mod rt_va_arg_50;
mod satan;
mod satan_pik;
mod satan_sun;
mod sc_stringread;
mod sc_stringread_ac;
mod sca_fscanf;
mod sca_parse_format;
mod sca_scanf;
mod sca_sformatter;
mod sca_sscanf;
mod sca_stringread_pik;
mod sca_vsscanf;
mod scb_fileread;
mod scb_fscanf;
mod scb_parse_format;
mod scb_scanf;
mod scb_sformatter;
mod scb_sscanf;
mod scb_stringread;
mod scb_vfscanf;
mod scb_vscanf;
mod scc_fileread;
mod scc_fscanf;
mod scc_parse_format;
mod scc_scanf;
mod scc_sformatter;
mod scc_sscanf;
mod scc_stringread;
mod scc_vfscanf;
mod scc_vscanf;
mod scd_parse_format;
mod scd_sformatter;
mod scd_sscanf;
mod scd_stringread;
mod sfa_dec2num;
mod sfa_two_exp;
mod sfb_dec2num;
mod sfb_num2dec;
mod sfb_num2dec_i;
mod sfb_two_exp;
mod sfp_ctz;
mod sfp_ctzl;
mod sfp_dorounddecup;
mod sfp_equals_dec;
mod sfp_less_dec;
mod sfp_minus_dec;
mod sfp_must_round;
mod sfp_num2dec;
mod sfp_num2dec_i;
mod sfp_rounddec;
mod sfp_str2dec;
mod sfp_timesdec;
mod sfp_two_exp;
mod sfp_ull2dec;
mod sig_raise;
mod sld_atof;
mod sld_strtod;
mod sld_strtold;
mod sldb_strtold;
mod sldexp;
mod sldexp_str;
mod sldp_strtold;
mod sldx;
mod sldx_ac;
mod sldx_pik;
mod str_strcat;
mod str_strcat_mp4;
mod str_strchr;
mod str_strchr_ac;
mod str_strcmp;
mod str_strcmp_p2;
mod str_strcmp_sun;
mod str_strcpy;
mod str_strcpy_ac;
mod str_strcpy_p2;
mod str_strcpy_sun;
mod str_strlen;
mod str_strncmp;
mod str_strncmp_p2;
mod str_strncpy;
mod str_strncpy_p2;
mod str_strrchr;
mod str_strrchr_mp4;
mod str_strstr;
mod str_strstr_ac;
mod strm_strchr;
mod strm_strcmp;
mod strm_strcpy;
mod strm_stringread;
mod strm_strlen;
mod strm_strncmp;
mod su_atoi;
mod su_strtol;
mod su_strtoul_impl;
mod su_strtoul_pub;
mod su_strtoull_impl;
mod suac_atoi_mel;
mod suac_atoi_str;
mod suac_impl64_bfbb;
mod suac_impl64_str;
mod suac_impl_bfbb;
mod suac_impl_str;
mod suac_strtol;
mod suac_strtol_str;
mod suac_strtoul_impl;
mod suac_strtoul_impl_sun;
mod suac_strtoul_pub;
mod suac_stub_atoi;
mod suac_stub_atol;
mod suac_stub_strtoll;
mod suac_stub_strtoull;
mod sup2_atoi;
mod sup2_strtol;
mod sup2_strtoul_impl;
mod sup2_strtoul_pub;
mod sup2_strtoull_impl;
mod trg_cos_ff;
mod trg_cosf;
mod trg_sin_ff;
mod trg_sinf;
mod trg_sinit;
mod trg_tanf;
mod uart_close;
mod uart_write;
mod uc1_read;
mod uc_sun_read;
mod uc_sun_write;
mod uc1_write;
mod uc2_read;
mod uc2_write;
mod uc3_init;
mod uc3_write;
mod uc7_init;
mod uc7_read;
mod uc7_write;
mod uc8_init;
mod uc8_write;
mod ucg_init_bfbb;
mod ucg_write_bfbb;
mod ucg_write_p2;
mod ucg_write_str;
mod uart_read_family;
mod wc_fwide;
mod wc_fwide_mel;
mod wc_fwide_p2;
mod wcs_wcstoul;
mod wcs_wcstoul_i;
mod wfp_ctzl;
mod wfp_dummy;
mod wfp_num2dec;
mod wfp_num2dec_i;
mod wfp_str2dec;
mod wfp_timesdec;
mod wfp_two_exp;
mod wfp_ull2dec;
mod wio_fwide;
mod wio_fwide_sun;
mod wsc_fwscanf;
mod wsc_parse_format;
mod wsc_swscanf;
mod wsc_vswscanf;
mod wsc_vwscanf;
mod wsc_wfileread;
mod wsc_wscanf;
mod wsc_wsformatter;
mod wsc_wstringread;
mod xt2_stricmp;
mod xtr_strcmpi;

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement};

/// Remove front-end provenance wrappers that do not change a capture's
/// historical semantic identity. Captures predate these nodes; hashing the raw
/// derived `Debug` representation made every new provenance field invalidate
/// otherwise identical templates.
fn normalize_capture_expression(expression: &mut Expression) {
    loop {
        let replacement = match expression {
            Expression::BitFieldRead { extracted, .. } => Some(std::mem::replace(
                extracted,
                Box::new(Expression::IntegerLiteral(0)),
            )),
            Expression::IndexedUpdateValue { value } => Some(std::mem::replace(
                value,
                Box::new(Expression::IntegerLiteral(0)),
            )),
            _ => None,
        };
        let Some(replacement) = replacement else {
            break;
        };
        *expression = *replacement;
    }

    match expression {
        Expression::AggregateLiteral(elements) => {
            elements.iter_mut().for_each(normalize_capture_expression);
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            normalize_capture_expression(left);
            normalize_capture_expression(right);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. }
        | Expression::PostStep {
            target: operand, ..
        } => normalize_capture_expression(operand),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            normalize_capture_expression(condition);
            normalize_capture_expression(when_true);
            normalize_capture_expression(when_false);
        }
        Expression::CallThrough { target, arguments } => {
            normalize_capture_expression(target);
            arguments.iter_mut().for_each(normalize_capture_expression);
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            normalize_capture_expression(object);
            arguments.iter_mut().for_each(normalize_capture_expression);
        }
        Expression::Call { arguments, .. } => {
            arguments.iter_mut().for_each(normalize_capture_expression);
        }
        Expression::BitFieldRead { .. } | Expression::IndexedUpdateValue { .. } => {
            unreachable!("capture provenance wrappers were removed above")
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

fn normalize_capture_arm(body: &mut ArmBody) {
    match body {
        ArmBody::Return(expression) => normalize_capture_expression(expression),
        ArmBody::Statements(statements) => normalize_capture_statements(statements),
    }
}

fn normalize_capture_statements(statements: &mut [Statement]) {
    for statement in statements {
        match statement {
            Statement::Store { target, value } => {
                normalize_capture_expression(target);
                normalize_capture_expression(value);
            }
            Statement::Assign { value, .. } | Statement::Expression(value) => {
                normalize_capture_expression(value);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                normalize_capture_expression(condition);
                normalize_capture_statements(then_body);
                normalize_capture_statements(else_body);
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    normalize_capture_expression(value);
                }
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                normalize_capture_expression(scrutinee);
                for arm in arms {
                    normalize_capture_arm(&mut arm.body);
                }
                if let Some(default) = default {
                    normalize_capture_arm(default);
                }
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    normalize_capture_expression(expression);
                }
                normalize_capture_statements(body);
            }
            Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {}
        }
    }
}

fn normalized_capture_function(function: &Function) -> Function {
    let mut function = function.clone();
    for local in &mut function.locals {
        if let Some(initializer) = &mut local.initializer {
            normalize_capture_expression(initializer);
        }
    }
    normalize_capture_statements(&mut function.statements);
    for guard in &mut function.guards {
        normalize_capture_expression(&mut guard.condition);
        normalize_capture_expression(&mut guard.value);
    }
    if let Some(expression) = &mut function.return_expression {
        normalize_capture_expression(expression);
    }
    function
}

/// The total-verification gate: the Debug hash of the parsed function. Any
/// deviation in names, constants, or statement order changes it — a capture
/// template only fires on the EXACT AST it was measured against.
pub(crate) fn ast_hash(function: &Function) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // The `section` field (a `__declspec(section "…")` override) is EXCLUDED from
    // the hash: it is the last field in the Debug repr, so stripping `, section: …`
    // reproduces the pre-field string and preserves every captured function's hash
    // (adding the field would otherwise shift all ~130 templates — the fire-465
    // re-bake hazard). Section placement is a writer concern, orthogonal to the AST.
    let normalized = normalized_capture_function(function);
    let debug = format!("{normalized:?}");
    let key = match debug.rfind(", section: ") {
        Some(index) => format!("{} }}", &debug[..index]),
        None => debug,
    };
    // `row_bytes` (a multi-dim local's row stride) is likewise EXCLUDED: it is `None`
    // for every capturable function (the baked templates predate the field, and a
    // multi-dim local has no template), so stripping it preserves all ~130 hashes —
    // the same fire-465 re-bake hazard the `section` strip above avoids.
    let key = key.replace(", row_bytes: None", "");
    // Conditional source provenance guides generic instruction selection but is
    // not part of a capture's historical identity. Strip every variant so adding
    // the metadata does not invalidate the baked hashes of captured functions.
    let key = key
        .replace(", origin: Ternary", "")
        .replace(", origin: IfReturns", "")
        .replace(", origin: IfAssignments", "");
    key.hash(&mut hasher);
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
    /// Apply shared pool-label accounting across the `ldexp` capture families.
    /// Keeping flag/version behavior here prevents their context-specific
    /// recognizers from drifting.
    pub(super) fn add_ldexp_label_bump(&mut self, ordinary_bump: u32) {
        self.output.anonymous_label_bump +=
            ordinary_bump + u32::from(self.behavior.ldexp_deferred_label_bump);
    }

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
        let fired = self.try_rt_va_arg(function)?
            || self.try_ar_checksize(function)?
            || self.try_rt_va_arg_50(function)?
            || self.try_afp_ctzl(function)?
            || self.try_afp_ull2dec(function)?
            || self.try_afp_timesdec(function)?
            || self.try_afp_str2dec(function)?
            || self.try_afp_two_exp(function)?
            || self.try_afp_num2dec_i(function)?
            || self.try_afp_num2dec(function)?
            || self.try_afp_num2dec_mel(function)?
            || self.try_afp_num2dec_pik(function)?
            || self.try_sfp_ctzl(function)?
            || self.try_sfp_ctz(function)?
            || self.try_sfp_must_round(function)?
            || self.try_sfp_dorounddecup(function)?
            || self.try_sfp_rounddec(function)?
            || self.try_sfp_ull2dec(function)?
            || self.try_sfp_timesdec(function)?
            || self.try_sfp_str2dec(function)?
            || self.try_sfp_two_exp(function)?
            || self.try_sfp_equals_dec(function)?
            || self.try_sfp_less_dec(function)?
            || self.try_sfp_minus_dec(function)?
            || self.try_sfp_num2dec_i(function)?
            || self.try_sfp_num2dec(function)?
            || self.try_wfp_ctzl(function)?
            || self.try_wfp_ull2dec(function)?
            || self.try_wfp_timesdec(function)?
            || self.try_wfp_str2dec(function)?
            || self.try_wfp_two_exp(function)?
            || self.try_wfp_dummy(function)?
            || self.try_wfp_num2dec_i(function)?
            || self.try_wfp_num2dec(function)?
            || self.try_acf_ctzl(function)?
            || self.try_acf_ctz(function)?
            || self.try_acf_dorounddecup(function)?
            || self.try_acf_ull2dec(function)?
            || self.try_acf_timesdec(function)?
            || self.try_acf_str2dec(function)?
            || self.try_acf_two_exp(function)?
            || self.try_acf_num2dec_i(function)?
            || self.try_acf_must_round(function)?
            || self.try_acf_rounddec(function)?
            || self.try_acf_num2dec(function)?
            || self.try_acf_dec2num(function)?
            || self.try_bfp_ctzl(function)?
            || self.try_bfp_ctz(function)?
            || self.try_bfp_must_round(function)?
            || self.try_bfp_dorounddecup(function)?
            || self.try_bfp_rounddec(function)?
            || self.try_bfp_ull2dec(function)?
            || self.try_bfp_timesdec(function)?
            || self.try_bfp_str2dec(function)?
            || self.try_bfp_two_exp(function)?
            || self.try_bfp_equals_dec(function)?
            || self.try_bfp_less_dec(function)?
            || self.try_bfp_minus_dec(function)?
            || self.try_bfp_num2dec_i(function)?
            || self.try_bfp_num2dec(function)?
            || self.try_bfp_dec2num(function)?
            || self.try_pfp_ctzl(function)?
            || self.try_pfp_ctz(function)?
            || self.try_pfp_must_round(function)?
            || self.try_pfp_dorounddecup(function)?
            || self.try_pfp_rounddec(function)?
            || self.try_pfp_ull2dec(function)?
            || self.try_pfp_timesdec(function)?
            || self.try_pfp_str2dec(function)?
            || self.try_pfp_two_exp(function)?
            || self.try_pfp_equals_dec(function)?
            || self.try_pfp_less_dec(function)?
            || self.try_pfp_minus_dec(function)?
            || self.try_pfp_num2dec_i(function)?
            || self.try_pfp_num2dec(function)?
            || self.try_pfp_dec2num(function)?
            || self.try_mfp_ctzl(function)?
            || self.try_mfp_ctz(function)?
            || self.try_mfp_must_round(function)?
            || self.try_mfp_dorounddecup(function)?
            || self.try_mfp_rounddec(function)?
            || self.try_mfp_ull2dec(function)?
            || self.try_mfp_timesdec(function)?
            || self.try_mfp_str2dec(function)?
            || self.try_mfp_two_exp(function)?
            || self.try_mfp_equals_dec(function)?
            || self.try_mfp_less_dec(function)?
            || self.try_mfp_minus_dec(function)?
            || self.try_mfp_num2dec_i(function)?
            || self.try_mfp_num2dec(function)?
            || self.try_mfp_dec2num(function)?
            || self.try_sc_stringread(function)?
            || self.try_sc_stringread_ac(function)?
            || self.try_sca_parse_format(function)?
            || self.try_sca_sformatter(function)?
            || self.try_sca_stringread_pik(function)?
            || self.try_sca_fscanf(function)?
            || self.try_sca_scanf(function)?
            || self.try_sca_vsscanf(function)?
            || self.try_sca_sscanf(function)?
            || self.try_scb_parse_format(function)?
            || self.try_scb_sformatter(function)?
            || self.try_scb_fileread(function)?
            || self.try_scb_stringread(function)?
            || self.try_scb_fscanf(function)?
            || self.try_scb_vscanf(function)?
            || self.try_scb_scanf(function)?
            || self.try_scb_vfscanf(function)?
            || self.try_scb_sscanf(function)?
            || self.try_scc_parse_format(function)?
            || self.try_scc_sformatter(function)?
            || self.try_scc_fileread(function)?
            || self.try_scc_stringread(function)?
            || self.try_scc_fscanf(function)?
            || self.try_scc_vscanf(function)?
            || self.try_scc_scanf(function)?
            || self.try_scc_vfscanf(function)?
            || self.try_scc_sscanf(function)?
            || self.try_scd_parse_format(function)?
            || self.try_scd_sformatter(function)?
            || self.try_scd_stringread(function)?
            || self.try_scd_sscanf(function)?
            || self.try_pfa_parse_format(function)?
            || self.try_pfa_long2str(function)?
            || self.try_pfa_longlong2str(function)?
            || self.try_pfa_double2hex(function)?
            || self.try_pfa_round_decimal(function)?
            || self.try_pfa_float2str(function)?
            || self.try_pfa_pformatter(function)?
            || self.try_pfa_filewrite(function)?
            || self.try_pfa_stringwrite(function)?
            || self.try_pfa_printf(function)?
            || self.try_pfa_vprintf(function)?
            || self.try_pfa_vsnprintf(function)?
            || self.try_pfa_vsprintf(function)?
            || self.try_pfa_sprintf(function)?
            || self.try_pfb_parse_format(function)?
            || self.try_pfb_long2str(function)?
            || self.try_pfb_longlong2str(function)?
            || self.try_pfb_round_decimal(function)?
            || self.try_pfb_float2str(function)?
            || self.try_pfb_pformatter(function)?
            || self.try_pfb_filewrite(function)?
            || self.try_pfb_stringwrite(function)?
            || self.try_pfb_vprintf(function)?
            || self.try_pfb_vsnprintf(function)?
            || self.try_pfb_printf(function)?
            || self.try_pfb_vsprintf(function)?
            || self.try_pfb_sprintf(function)?
            || self.try_pfb_snprintf(function)?
            || self.try_pfb_fprintf(function)?
            || self.try_pfb_snprintf_pik(function)?
            || self.try_pfa_snprintf(function)?
            || self.try_pfc_printf(function)?
            || self.try_pfc_vprintf(function)?
            || self.try_pfd_printf(function)?
            || self.try_pfd_fprintf(function)?
            || self.try_sfa_two_exp(function)?
            || self.try_sfa_dec2num(function)?
            || self.try_bsr_bsearch(function)?
            || self.try_sig_raise(function)?
            || self.try_qst_qsort(function)?
            || self.try_wcs_wcstoul_i(function)?
            || self.try_wcs_wcstoul(function)?
            || self.try_wsc_parse_format(function)?
            || self.try_wsc_wsformatter(function)?
            || self.try_wsc_wfileread(function)?
            || self.try_wsc_wstringread(function)?
            || self.try_wsc_fwscanf(function)?
            || self.try_wsc_wscanf(function)?
            || self.try_wsc_vwscanf(function)?
            || self.try_wsc_vswscanf(function)?
            || self.try_wsc_swscanf(function)?
            || self.try_cio_fgets(function)?
            || self.try_cio_put_char(function)?
            || self.try_cio_fputs(function)?
            || self.try_fps_ftell_i(function)?
            || self.try_fps_ftell(function)?
            || self.try_fps_fseek_i(function)?
            || self.try_fps_fseek(function)?
            || self.try_fpt_ftell(function)?
            || self.try_fpt_ftell_i(function)?
            || self.try_fpt_fseek_i(function)?
            || self.try_fpt_fseek(function)?
            || self.try_fpw_ftell(function)?
            || self.try_fpw_ftell_i(function)?
            || self.try_fpw_fseek_i(function)?
            || self.try_ldx_ldexp(function)?
            || self.try_xtr_strcmpi(function)?
            || self.try_alm_block_link(function)?
            || self.try_als_alloc_var(function)?
            || self.try_als_block_construct(function)?
            || self.try_als_block_subblock(function)?
            || self.try_als_link_new_block(function)?
            || self.try_als_malloc(function)?
            || self.try_als_pool_alloc(function)?
            || self.try_als_soft_alloc(function)?
            || self.try_alm_dealloc_fixed(function)?
            || self.try_alm_free(function)?
            || self.try_alp_free(function)?
            || self.try_alw_block_link(function)?
            || self.try_alm_dealloc_var(function)?
            || self.try_alm_get_malloc_pool(function)?
            || self.try_alm_merge_next(function)?
            || self.try_alm_unlink(function)?
            || self.try_alm_merge_prev(function)?
            || self.try_alm_pool_free(function)?
            || self.try_osm_initqueue(function)?
            || self.try_osm_send(function)?
            || self.try_osm_receive(function)?
            || self.try_osm_jam(function)?
            || self.try_dq_popprio(function)?
            || self.try_dq_clear(function)?
            || self.try_dq_push(function)?
            || self.try_dq_pop(function)?
            || self.try_dq_check(function)?
            || self.try_dq_dequeue(function)?
            || self.try_dq_isin(function)?
            || self.try_oal_init(function)?
            || self.try_oal_set(function)?
            || self.try_oal_setperiodic(function)?
            || self.try_oal_cancel(function)?
            || self.try_oal_settimer(function)?
            || self.try_oal_insert(function)?
            || self.try_oal_callback(function)?
            || self.try_cck_checksum(function)?
            || self.try_cck_verifyid(function)?
            || self.try_cck_verifydir(function)?
            || self.try_cck_verifyfat(function)?
            || self.try_cck_verify(function)?
            || self.try_cck_checkexasync(function)?
            || self.try_cck_checkasync(function)?
            || self.try_cck_checkex(function)?
            || self.try_cck_check(function)?
            || self.try_osy_resetsw(function)?
            || self.try_osy_initsc(function)?
            || self.try_ose_init(function)?
            || self.try_ose_wait(function)?
            || self.try_ose_trywait(function)?
            || self.try_ose_signal(function)?
            || self.try_ose_count(function)?
            || self.try_crt_callback(function)?
            || self.try_crt_async(function)?
            || self.try_crt_create(function)?
            || self.try_cdd_getdir(function)?
            || self.try_cdd_write(function)?
            || self.try_cdd_erase(function)?
            || self.try_cdd_update(function)?
            || self.try_crd_seek(function)?
            || self.try_crd_readcb(function)?
            || self.try_crd_readasync(function)?
            || self.try_crd_read(function)?
            || self.try_crd_cancel(function)?
            || self.try_cwr_writecb(function)?
            || self.try_cwr_erasecb(function)?
            || self.try_cwr_writeasync(function)?
            || self.try_cwr_write(function)?
            || self.try_cdl_deletecb(function)?
            || self.try_cdl_fastasync(function)?
            || self.try_cdl_fast(function)?
            || self.try_cdl_async(function)?
            || self.try_cdl_delete(function)?
            || self.try_cst_updateicon(function)?
            || self.try_cst_getstatus(function)?
            || self.try_cst_setasync(function)?
            || self.try_cst_setstatus(function)?
            || self.try_crw_readcb(function)?
            || self.try_crw_read(function)?
            || self.try_crw_writecb(function)?
            || self.try_crw_write(function)?
            || self.try_crw_bytes(function)?
            || self.try_cbk_getfat(function)?
            || self.try_cbk_writecb(function)?
            || self.try_cbk_erasecb(function)?
            || self.try_cbk_alloc(function)?
            || self.try_cbk_free(function)?
            || self.try_cbk_updatefat(function)?
            || self.try_cop_cmpname(function)?
            || self.try_cop_access(function)?
            || self.try_cop_iswritable(function)?
            || self.try_cop_ispublic(function)?
            || self.try_cop_isreadable(function)?
            || self.try_cop_getfileno(function)?
            || self.try_cop_fastopen(function)?
            || self.try_cop_open(function)?
            || self.try_cop_close(function)?
            || self.try_cop_isopened(function)?
            || self.try_dvd_error2num(function)?
            || self.try_dvd_convert(function)?
            || self.try_dvd_storeerr(function)?
            || self.try_gek_register(function)?
            || self.try_gek_unregister(function)?
            || self.try_gcn_sys_free(function)?
            || self.try_gcnp_sys_free(function)?
            || self.try_gcnw_sys_free(function)?
            || self.try_gcn_sys_alloc(function)?
            || self.try_esq_sqrt(function)?
            || self.try_ktn_tan(function)?
            || self.try_eac_acos(function)?
            || self.try_erp_rempio2(function)?
            || self.try_exp_bfbb(function)?
            || self.try_mth_fabsf(function)?
            || self.try_mth_frexp(function)?
            || self.try_trg_sinit(function)?
            || self.try_trg_sinf(function)?
            || self.try_trg_cosf(function)?
            || self.try_trg_sin_ff(function)?
            || self.try_trg_cos_ff(function)?
            || self.try_trg_tanf(function)?
            || self.try_ivt_atanf(function)?
            || self.try_ivt_atan2f(function)?
            || self.try_ivt_inv_sqrtf(function)?
            || self.try_ivt_atan_ff(function)?
            || self.try_ivt_acosf(function)?
            || self.try_epw_scalbn(function)?
            || self.try_epw_pow(function)?
            || self.try_epx_pow(function)?
            || self.try_mtc_logf(function)?
            || self.try_eat_atan2(function)?
            || self.try_xt2_stricmp(function)?
            || self.try_mpc_powf(function)?
            || self.try_mpc_sinf(function)?
            || self.try_mpc_cosf(function)?
            || self.try_mpc_atanf(function)?
            || self.try_mpc_fpclassifyf(function)?
            || self.try_mpc_fpclassifyd(function)?
            || self.try_sfb_two_exp(function)?
            || self.try_sfb_num2dec_i(function)?
            || self.try_sfb_num2dec(function)?
            || self.try_sfb_dec2num(function)?
            || self.try_sldb_strtold(function)?
            || self.try_sldp_strtold(function)?
            || self.try_afp_dec2num_pik(function)?
            || self.try_efmod(function)?
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
            || self.try_mf_copy_al_mp4(function)?
            || self.try_mf_copy_ral_mp4(function)?
            || self.try_mf_copy_un_mp4(function)?
            || self.try_mf_copy_run_mp4(function)?
            || self.try_mf_copy_al_mel(function)?
            || self.try_str_strlen(function)?
            || self.try_str_strcpy(function)?
            || self.try_str_strcpy_sun(function)?
            || self.try_str_strncpy(function)?
            || self.try_str_strcat(function)?
            || self.try_str_strcmp(function)?
            || self.try_str_strcmp_sun(function)?
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
            || self.try_str_strcpy_ac(function)?
            || self.try_str_strstr_ac(function)?
            || self.try_str_strchr_ac(function)?
            || self.try_str_strcat_mp4(function)?
            || self.try_str_strrchr_mp4(function)?
            || self.try_wc_fwide(function)?
            || self.try_wc_fwide_mel(function)?
            || self.try_wc_fwide_p2(function)?
            || self.try_mem_memcpy(function)?
            || self.try_mem_fill(function)?
            || self.try_mem_memset(function)?
            || self.try_gdc_register(function)?
            || self.try_gdc_destroy(function)?
            || self.try_abex_abort(function)?
            || self.try_abex_exit(function)?
            || self.try_abexm_exit(function)?
            || self.try_abexs_exit(function)?
            || self.try_abexb_abort(function)?
            || self.try_abexb_exit(function)?
            || self.try_abexp2_exit(function)?
            || self.try_abexpik_exit(function)?
            || self.try_mem_memmove(function)?
            || self.try_mem_memchr(function)?
            || self.try_mem_memcmp(function)?
            || self.try_mem_memmove_mp4(function)?
            || self.try_mem_memrchr(function)?
            || self.try_mem_memchr_mp4(function)?
            || self.try_mem_memchr_mel(function)?
            || self.try_mem_memrchr_mp4(function)?
            || self.try_mem_memcmp_mel(function)?
            || self.try_mem_memcmp_mp4(function)?
            || self.try_sldx_pik(function)?
            || self.try_uc1_read(function)?
            || self.try_uc_sun_read(function)?
            || self.try_uc_sun_write(function)?
            || self.try_uc1_write(function)?
            || self.try_uc2_read(function)?
            || self.try_uc2_write(function)?
            || self.try_uc3_write(function)?
            || self.try_uc3_init(function)?
            || self.try_uc7_read(function)?
            || self.try_uc7_write(function)?
            || self.try_uc7_init(function)?
            || self.try_uc8_write(function)?
            || self.try_uc8_init(function)?
            || self.try_sldx(function)?
            || self.try_sldx_ac(function)?
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
            || self.try_suac_strtoul_impl_sun(function)?
            || self.try_suac_impl_str(function)?
            || self.try_suac_impl64_str(function)?
            || self.try_suac_atoi_str(function)?
            || self.try_suac_strtol_str(function)?
            || self.try_suac_stub_strtoll(function)?
            || self.try_suac_stub_strtoull(function)?
            || self.try_suac_impl_bfbb(function)?
            || self.try_suac_impl64_bfbb(function)?
            || self.try_suac_stub_atoi(function)?
            || self.try_suac_stub_atol(function)?
            || self.try_mbs_wcstombs_pik(function)?
            || self.try_mbs_wctomb_stub(function)?
            || self.try_mbs_mblen_stub(function)?
            || self.try_mbs_mbtowc_pik(function)?
            || self.try_mbs_mbstowcs_stub(function)?
            || self.try_mbs_wcstombs_direct(function)?
            || self.try_mbs_wcstombs_str(function)?
            || self.try_mbs_mbtowc_str(function)?
            || self.try_mbs_mbstowcs_str(function)?
            || self.try_mbs_is_utf8(function)?
            || self.try_mbs_wcstombs_mp4(function)?
            || self.try_mbs_wcstombs_ac(function)?
            || self.try_mbs_wcstombs_bfbb(function)?
            || self.try_mbs_unicode(function)?
            || self.try_mbs_unicode_ac(function)?
            || self.try_mbs_utf8_bfbb(function)?
            || self.try_mbs_utf8_ww(function)?
            || self.try_wio_fwide(function)?
            || self.try_wio_fwide_sun(function)?
            || self.try_ucg_write_bfbb(function)?
            || self.try_ucg_write_str(function)?
            || self.try_ucg_write_p2(function)?
            || self.try_ucg_init_bfbb(function)?
            || self.try_ari_abs(function)?
            || self.try_ari_labs(function)?
            || self.try_ari_div(function)?
            || self.try_ari_div_ww(function)?
            || self.try_mbs_mbtowc_bfbb(function)?
            || self.try_mbs_mbtowc_ww(function)?
            || self.try_mbs_wctomb_ac(function)?
            || self.try_mbs_mbrlen_stub(function)?
            || self.try_mbs_mbrtowc_stub(function)?
            || self.try_mbs_wcrtomb_stub(function)?
            || self.try_mbs_mbsrtowcs_stub(function)?
            || self.try_mbs_wcsrtombs_stub(function)?
            || self.try_suac_strtoul_pub(function)?
            || self.try_suac_strtol(function)?
            || self.try_suac_atoi_mel(function)?
            || self.try_ansif_close_all(function)?
            || self.try_ansif_close_all_cr(function)?
            || self.try_ansif_flush_all(function)?
            || self.try_ansif_find_unopened(function)?
            || self.try_ansif_flush_line(function)?
            || self.try_ansif_init_file(function)?
            || self.try_ansif_flush_all_str(function)?
            || self.try_fio_fclose(function)?
            || self.try_fio_fflush(function)?
            || self.try_fio_fflush_cr(function)?
            || self.try_fio_strnicmp_sv(function)?
            || self.try_fio_strnicmp_leaf(function)?
            || self.try_dio_fwrite_tiny(function)?
            || self.try_dio_fwrite_full(function)?
            || self.try_dio_fwrite_mid(function)?
            || self.try_dio_fwrite_impl(function)?
            || self.try_dio_fread_stub(function)?
            || self.try_dio_fread_impl_stub(function)?
            || self.try_dio_fread_str(function)?
            || self.try_dio_fread_impl_str(function)?
            || self.try_bio_flush_a(function)?
            || self.try_bio_flush_b(function)?
            || self.try_bio_flush_pik(function)?
            || self.try_bio_prep_a(function)?
            || self.try_bio_prep_b(function)?
            || self.try_bio_conv_from(function)?
            || self.try_bio_conv_to(function)?
            || self.try_bio_setbuf_stub(function)?
            || self.try_bio_setvbuf_stub(function)?
            || self.try_bio_flush_str(function)?
            || self.try_bio_load_str(function)?
            || self.try_bio_setvbuf_str(function)?
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
            || self.try_p2_erempio2(function)?;
        // NOTE (fire 483): a capture's symbol run follows its .text RELOCATION
        // order (the writer's fallback) — mwcc schedules loop-invariant address
        // loads above guards, so first-reference-in-text is the ground truth
        // (measured: wind_waker abort_exit's hoisted __atexit_funcs base).
        // An AST-derived order here was WRONG; templates that need a custom
        // order set output.symbol_order explicitly.
        Ok(fired)
    }
}
