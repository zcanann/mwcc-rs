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
    /// `-inline …,deferred`: deferred inlining emits the object's compiler-generated
    /// functions in REVERSE definition order (and thus their symbols/records too).
    /// Hand-written `asm` functions are emitted immediately and retain source order.
    pub inline_deferred: bool,
    /// Whether C++ exception support is on (the default). `-Cpp_exceptions off`
    /// suppresses the `extab`/`extabindex` unwind tables entirely (the stack frame
    /// itself is unchanged).
    pub cpp_exceptions: bool,
    /// `-pragma "cats off"` disables Code Address Table emission. The functions
    /// remain in `.text`, but the `.mwcats.text` catalog and its relocations are
    /// absent from the object.
    pub emit_mwcats: bool,
    /// `-str …,readonly` places pooled string literals in a read-only data
    /// section rather than writable `.data`.
    pub string_literals_read_only: bool,
    /// Whether compiler pooling is enabled. The verified object-level effect is
    /// byte 16 of the `.comment` header; pooling passes consume this same mode.
    pub pooling_enabled: bool,
}

impl Default for Flags {
    /// The configuration the canary corpus is built with: `-O4,p`, small-data on,
    /// plain `char` per the build default, immediate (non-deferred) inlining.
    fn default() -> Self {
        Flags {
            optimization: Optimization::O4,
            global_addressing: GlobalAddressing::SmallData,
            char_default: CharDefault::BuildDefault,
            inline_deferred: false,
            cpp_exceptions: true,
            emit_mwcats: true,
            string_literals_read_only: false,
            pooling_enabled: true,
        }
    }
}
