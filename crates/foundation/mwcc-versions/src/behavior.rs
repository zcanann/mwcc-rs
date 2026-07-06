//! Resolved codegen behavior: the single, inspectable set of decisions a
//! [`CompilerConfig`] (a build plus its flags) implies for the code generator.
//!
//! The pipeline never reaches into a build's profile or pokes at flags while
//! emitting code. It resolves one [`Behavior`] up front and reads named
//! decisions from it. The decisions that vary across builds are surfaced as
//! [`Quirk`]s, each carrying not just a value but *why* it exists — a deliberate
//! version-to-version design change, or the faithful reproduction of a real
//! compiler bug. That makes divergences enumerable: a configuration's active
//! quirks can be listed and explained ([`Behavior::active_quirks`]), so
//! reproducing a compiler bug is a deliberate, visible act rather than a magic
//! constant buried in instruction selection.

use crate::config::CompilerConfig;
use crate::flags::GlobalAddressing;

/// Why a codegen decision diverges from the GameCube 2.4.x mainline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuirkKind {
    /// A deliberate design change between versions — e.g. the plain-`char`
    /// signedness default that build 81 flipped back to signed.
    Intentional,
    /// A faithful reproduction of an actual compiler bug or accident: behavior
    /// that is "wrong" in isolation but must be matched to reproduce the
    /// original bytes. Kept distinct from [`QuirkKind::Intentional`] so bug
    /// emulation is always an explicit, documented choice.
    BugReproduction,
}

/// A named codegen decision that diverges from the mainline for some builds. The
/// set is closed (an enum) so every divergence has a stable identity that can be
/// listed, explained, and asserted against in tests. Each variant names the
/// *non-default* behavior; it is "active" exactly when a configuration exhibits
/// it (see [`Behavior::active_quirks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quirk {
    /// Plain `char` (no `signed`/`unsigned` qualifier) defaults to *unsigned*
    /// rather than signed. GameCube build 53, and any `-char unsigned`. The
    /// mainline (build 81+) treats plain `char` as signed.
    UnsignedPlainChar,
    /// The int->float conversion stores the integer value (`stw rX,12(r1)`)
    /// before loading the bias double (`lfd f1,0(0)`), reversing the mainline
    /// schedule. Unique to GC/2.0p1.
    FloatCastStoresValueFirst,
}

impl Quirk {
    /// Whether this quirk is a deliberate version difference or a reproduced bug.
    pub fn kind(self) -> QuirkKind {
        match self {
            // Build 81 deliberately restored signed `char`; 53 is the older design.
            Quirk::UnsignedPlainChar => QuirkKind::Intentional,
            // A scheduling change introduced by the 2.0 patch release.
            Quirk::FloatCastStoresValueFirst => QuirkKind::Intentional,
        }
    }

    /// A one-line human explanation, for inspection and the artifact dump.
    pub fn summary(self) -> &'static str {
        match self {
            Quirk::UnsignedPlainChar => "plain `char` defaults to unsigned (build 53 / -char unsigned)",
            Quirk::FloatCastStoresValueFirst => "int->float stores the value before loading the bias double (GC/2.0p1)",
        }
    }
}

/// The codegen decisions resolved from one [`CompilerConfig`]. This is the only
/// thing the code generator consults for version- and flag-varying behavior;
/// the build identity (version/build numbers, for object metadata) stays on the
/// config. Resolving once, here, keeps version checks out of instruction
/// selection — codegen reads a plain field, never a trait object or a flag.
#[derive(Debug, Clone, Copy)]
pub struct Behavior {
    /// Whether plain `char` is signed. Cascades through read/operand extension,
    /// `>>`/`/`/`%` strength reduction, comparison folding, and the int->float bias.
    pub char_is_signed: bool,
    /// In the int->float conversion, whether the value store is scheduled before
    /// the bias load (GC/2.0p1's order).
    pub float_cast_value_store_first: bool,
    /// In a non-leaf `if`-prologue, whether the saved-LR store precedes a leading
    /// float-constant load rather than filling the mflr->store latency slot with it
    /// (GC/2.0p1's order).
    pub lr_save_precedes_float_const: bool,
    /// How file-scope globals are addressed — small-data (SDA21 off r13) or
    /// absolute (ADDR16 hi/lo). Driven by `-sdata`; the resolved home for the
    /// addressing decision Phase C will consume.
    pub global_addressing: GlobalAddressing,
}

/// A quirk that is active for a configuration, paired with its kind and summary
/// so a caller can list and explain a build's divergences without re-deriving them.
#[derive(Debug, Clone, Copy)]
pub struct ActiveQuirk {
    pub quirk: Quirk,
    pub kind: QuirkKind,
    pub summary: &'static str,
}

impl ActiveQuirk {
    fn of(quirk: Quirk) -> Self {
        ActiveQuirk { quirk, kind: quirk.kind(), summary: quirk.summary() }
    }
}

impl Behavior {
    /// Resolve every codegen decision for `config`, collapsing the build's
    /// profile and the flags into one flat set of values.
    pub fn resolve(config: &CompilerConfig) -> Self {
        Behavior {
            char_is_signed: config.char_is_signed(),
            float_cast_value_store_first: config.build.profile.float_cast_value_store_first(),
            lr_save_precedes_float_const: config.build.profile.lr_save_precedes_float_const(),
            global_addressing: config.flags.global_addressing,
        }
    }

    /// The quirks that diverge from the mainline for this configuration, each
    /// with its kind and explanation. Empty for a plain mainline build — the
    /// list is exactly "what makes this configuration special".
    pub fn active_quirks(&self) -> Vec<ActiveQuirk> {
        let mut quirks = Vec::new();
        if !self.char_is_signed {
            quirks.push(ActiveQuirk::of(Quirk::UnsignedPlainChar));
        }
        if self.float_cast_value_store_first {
            quirks.push(ActiveQuirk::of(Quirk::FloatCastStoresValueFirst));
        }
        quirks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build, flags::CharDefault};

    #[test]
    fn mainline_has_no_active_quirks() {
        let behavior = Behavior::resolve(&CompilerConfig::new(build::GC_1_3_2));
        assert!(behavior.active_quirks().is_empty());
        assert!(behavior.char_is_signed);
    }

    #[test]
    fn build_53_reports_the_unsigned_char_quirk() {
        let behavior = Behavior::resolve(&CompilerConfig::new(build::GC_1_3));
        let quirks = behavior.active_quirks();
        assert_eq!(quirks.len(), 1);
        assert_eq!(quirks[0].quirk, Quirk::UnsignedPlainChar);
        assert_eq!(quirks[0].kind, QuirkKind::Intentional);
    }

    #[test]
    fn float_cast_quirk_is_unique_to_2_0p1() {
        let plain = Behavior::resolve(&CompilerConfig::new(build::GC_2_0));
        assert!(!plain.float_cast_value_store_first);
        let patched = Behavior::resolve(&CompilerConfig::new(build::GC_2_0P1));
        assert!(patched.float_cast_value_store_first);
        assert_eq!(patched.active_quirks()[0].quirk, Quirk::FloatCastStoresValueFirst);
    }

    #[test]
    fn char_flag_overrides_the_build_default_as_a_quirk() {
        let mut config = CompilerConfig::new(build::GC_1_3_2);
        config.flags.char_default = CharDefault::Unsigned;
        let behavior = Behavior::resolve(&config);
        assert!(!behavior.char_is_signed);
        assert_eq!(behavior.active_quirks()[0].quirk, Quirk::UnsignedPlainChar);
    }
}
