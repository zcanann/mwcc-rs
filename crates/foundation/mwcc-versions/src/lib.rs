//! The compiler-build registry.
//!
//! Byte-matching is per-build: "GC/1.3.2" is a label for mwcceppc internal
//! version 2.4.2 build 81, and its register allocator and scheduler differ from
//! adjacent builds. Codegen is parameterized by [`CompilerBuild`] so a single
//! source tree can target many versions; behavior differences are expressed as
//! explicit knobs rather than scattered version checks.
//!
//! Empirically (differential oracle + cross-build `tools/vdiff.sh` over ~320
//! probed forms), builds 53 through 108 of the GameCube line share one code
//! generator with exactly one observable knob: the default signedness of plain
//! `char`. Build 53 (GC/1.3) defaults it *unsigned*; build 81 (GC/1.3.2) and
//! every later build default it *signed*. That single flag cascades — through
//! shift/divide strength reduction, comparison folding, and the int->float bias
//! sequence — into all the char-related code differences we observe, because the
//! rest of the generator already lowers signed and unsigned values correctly.

/// A specific mwcceppc build we aim to reproduce byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerBuild {
    /// The decomp-community label, e.g. "GC/1.3.2".
    pub label: &'static str,
    /// Marketed product line, e.g. "CodeWarrior for GameCube 1.3.2".
    pub product: &'static str,
    /// Internal compiler version, e.g. (2, 4, 2).
    pub version: (u8, u8, u8),
    /// Internal build number, e.g. 81.
    pub build: u16,
    /// Whether plain `char` (no `signed`/`unsigned` qualifier) is signed.
    /// The lone codegen knob distinguishing GC builds 53..=108.
    pub char_is_signed: bool,
    /// In the int→float conversion, whether the value store (`stw rX,12(r1)`) is
    /// scheduled before the bias load (`lfd f1,0(0)`). GC/2.0p1 orders it this
    /// way; every other supported build loads the bias first. The first observed
    /// instruction-scheduling difference between builds.
    pub float_cast_value_store_first: bool,
}

/// GC/1.3 — mwcceppc 2.4.2 build 53. The earliest preserved 2.4.2 build; the
/// only one in the supported range that defaults plain `char` to *unsigned*.
pub const GC_1_3: CompilerBuild = CompilerBuild {
    label: "GC/1.3",
    product: "CodeWarrior for GameCube 1.3",
    version: (2, 4, 2),
    build: 53,
    char_is_signed: false,
    float_cast_value_store_first: false,
};

/// GC/1.3.2 — mwcceppc 2.4.2 build 81 (built 2002-05-07). The first target and
/// the build that restored signed-by-default `char`.
pub const GC_1_3_2: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2",
    product: "CodeWarrior for GameCube 1.3.2",
    version: (2, 4, 2),
    build: 81,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// GC/1.3.2r — mwcceppc 2.4.2 build 81, byte-identical to GC/1.3.2 across the
/// canary suite (a re-release of the same compiler).
pub const GC_1_3_2R: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2r",
    product: "CodeWarrior for GameCube 1.3.2 (r)",
    version: (2, 4, 2),
    build: 81,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// GC/2.0 — mwcceppc 2.4.7 build 92.
pub const GC_2_0: CompilerBuild = CompilerBuild {
    label: "GC/2.0",
    product: "CodeWarrior for GameCube 2.0",
    version: (2, 4, 7),
    build: 92,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// GC/2.0p1 — mwcceppc 2.4.7 build 92. Identical to GC/2.0 except the int→float
/// conversion schedules the value store before the bias load.
pub const GC_2_0P1: CompilerBuild = CompilerBuild {
    label: "GC/2.0p1",
    product: "CodeWarrior for GameCube 2.0 (patch 1)",
    version: (2, 4, 7),
    build: 92,
    char_is_signed: true,
    float_cast_value_store_first: true,
};

/// GC/2.5 — mwcceppc 2.4.7 build 105.
pub const GC_2_5: CompilerBuild = CompilerBuild {
    label: "GC/2.5",
    product: "CodeWarrior for GameCube 2.5",
    version: (2, 4, 7),
    build: 105,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// GC/2.6 — mwcceppc 2.4.7 build 107.
pub const GC_2_6: CompilerBuild = CompilerBuild {
    label: "GC/2.6",
    product: "CodeWarrior for GameCube 2.6",
    version: (2, 4, 7),
    build: 107,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// GC/2.7 — mwcceppc 2.4.7 build 108.
pub const GC_2_7: CompilerBuild = CompilerBuild {
    label: "GC/2.7",
    product: "CodeWarrior for GameCube 2.7",
    version: (2, 4, 7),
    build: 108,
    char_is_signed: true,
    float_cast_value_store_first: false,
};

/// Every build the generator reproduces byte-for-byte across the canary suite.
pub const SUPPORTED: &[CompilerBuild] = &[GC_1_3, GC_1_3_2, GC_1_3_2R, GC_2_0, GC_2_0P1, GC_2_5, GC_2_6, GC_2_7];

/// The default build new compilations target until one is selected.
pub const DEFAULT: CompilerBuild = GC_1_3_2;

/// Look up a build by its decomp label (e.g. "GC/1.3.2" or the bare "1.3.2").
pub fn by_label(label: &str) -> Option<CompilerBuild> {
    SUPPORTED
        .iter()
        .copied()
        .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
}
