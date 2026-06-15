//! Codegen-affecting compiler flags.
//!
//! A real build line — e.g. `-O0,p -sdata 0 -char unsigned -fp hardware` — selects
//! behaviors that change the emitted bytes. We model the flags that matter for
//! byte-matching here; everything else (warnings, include paths, defines) is the
//! preprocessor/diagnostics' concern and does not reach codegen.
//!
//! Defaults reproduce the configuration our canary corpus is built with
//! (`-O4,p -fp hardware`, small-data on), so threading `Flags` through the
//! pipeline is behavior-preserving until a flag is deliberately changed.

/// The optimization level (`-O0`..`-O4`). The trailing `,p`/`,s`/`,space` sub-mode
/// is tracked separately. `-O0` runs no optimizer (straight-line selection); the
/// higher levels enable reassociation, CSE, and the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Optimization {
    O0,
    O1,
    O2,
    O3,
    O4,
}

/// How file-scope globals are addressed. `-sdata N`/`-sdata2 N` set the small-data
/// thresholds; a threshold of zero means a symbol never lands in small data, so it
/// is addressed absolutely (`lis`/`addi` with `R_PPC_ADDR16_HA`/`_LO`) rather than
/// off `r13`/`r2` (`R_PPC_EMB_SDA21`). REL modules build with both at zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalAddressing {
    /// Small-data area: an `R_PPC_EMB_SDA21` reference off `r13`/`r2`.
    SmallData,
    /// Absolute: `lis hi; addi lo` with `R_PPC_ADDR16_HA`/`_LO`.
    Absolute,
}

/// Signedness of plain `char`. A build has a default (see
/// [`crate::CodegenProfile::char_is_signed`]); `-char signed`/`-char unsigned`
/// overrides it. `None` means "use the build default".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharDefault {
    BuildDefault,
    Signed,
    Unsigned,
}

/// The codegen-affecting flags of one invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub optimization: Optimization,
    pub global_addressing: GlobalAddressing,
    pub char_default: CharDefault,
}

impl Default for Flags {
    /// The configuration the canary corpus is built with: `-O4,p`, small-data on,
    /// plain `char` per the build default.
    fn default() -> Self {
        Flags {
            optimization: Optimization::O4,
            global_addressing: GlobalAddressing::SmallData,
            char_default: CharDefault::BuildDefault,
        }
    }
}
