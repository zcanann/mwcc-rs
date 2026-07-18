//! The build registry: one [`CompilerBuild`] per mwcceppc build we reproduce,
//! each pointing at the [`CodegenProfile`] that says how its codegen diverges
//! from the mainline. Adding a build is one entry here; adding a *behavior* is a
//! profile in [`crate::profile`].

use crate::profile::{CodegenProfile, Gc13Build53, Gc20Patch1, Gc233Build163, Mainline};

/// A specific mwcceppc build we aim to reproduce byte-for-byte.
#[derive(Debug, Clone, Copy)]
pub struct CompilerBuild {
    /// The decomp-community label, e.g. "GC/1.3.2".
    pub label: &'static str,
    /// Marketed product line, e.g. "CodeWarrior for GameCube 1.3.2".
    pub product: &'static str,
    /// Internal compiler version, e.g. (2, 4, 2).
    pub version: (u8, u8, u8),
    /// Internal build number, e.g. 81.
    pub build: u16,
    /// Marker byte used by this compiler's Metrowerks `.comment` record.
    pub comment_marker: u8,
    /// Version tuple encoded in `.comment`.  This is deliberately separate
    /// from `version`: legacy 2.3.3 executables identify their object format as
    /// 2.3.0.
    pub comment_version: (u8, u8, u8),
    /// Byte offset within an instruction used by SDA21 relocation records.
    pub emb_sda21_offset: u8,
    /// Whether a global function symbol precedes externals first referenced by
    /// its body (2.3.3), instead of following prototyped externals (2.4.x).
    pub function_symbol_before_references: bool,
    /// Initial value of the translation unit's anonymous `@N` counter.
    pub initial_anonymous_counter: u8,
    /// Anonymous-ordinal gap inserted after a frameless leaf function.
    pub post_leaf_function_anonymous_bump: u8,
    /// Anonymous-ordinal gap inserted after a function with unwind metadata.
    pub post_framed_function_anonymous_bump: u8,
    /// How this build's code generation diverges from the 2.4.x mainline.
    pub profile: &'static dyn CodegenProfile,
}

/// GC/1.2.5 — mwcceppc 2.3.3 build 163. Kept experimental until its complete
/// frame scheduler and object conventions pass the full corpus.
pub const GC_1_2_5: CompilerBuild = CompilerBuild {
    label: "GC/1.2.5",
    product: "CodeWarrior for GameCube 1.2.5",
    version: (2, 3, 3),
    build: 163,
    comment_marker: 0x08,
    comment_version: (2, 3, 0),
    emb_sda21_offset: 2,
    function_symbol_before_references: true,
    initial_anonymous_counter: 2,
    post_leaf_function_anonymous_bump: 1,
    post_framed_function_anonymous_bump: 1,
    profile: &Gc233Build163,
};

/// GC/1.2.5n — Nintendo's distribution of the same compiler build.
pub const GC_1_2_5N: CompilerBuild = CompilerBuild {
    label: "GC/1.2.5n",
    product: "CodeWarrior for GameCube 1.2.5 (Nintendo)",
    version: (2, 3, 3),
    build: 163,
    comment_marker: 0x08,
    comment_version: (2, 3, 0),
    emb_sda21_offset: 2,
    function_symbol_before_references: true,
    initial_anonymous_counter: 2,
    post_leaf_function_anonymous_bump: 1,
    post_framed_function_anonymous_bump: 1,
    profile: &Gc233Build163,
};

/// GC/1.3 — mwcceppc 2.4.2 build 53. The earliest preserved 2.4.2 build; the
/// only one in the supported range that defaults plain `char` to unsigned.
pub const GC_1_3: CompilerBuild = CompilerBuild {
    label: "GC/1.3",
    product: "CodeWarrior for GameCube 1.3",
    version: (2, 4, 2),
    build: 53,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc13Build53,
};

/// GC/1.3.2 — mwcceppc 2.4.2 build 81 (built 2002-05-07), the reference build.
pub const GC_1_3_2: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2",
    product: "CodeWarrior for GameCube 1.3.2",
    version: (2, 4, 2),
    build: 81,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/1.3.2r — mwcceppc 2.4.2 build 81, byte-identical to GC/1.3.2 (a re-release).
pub const GC_1_3_2R: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2r",
    product: "CodeWarrior for GameCube 1.3.2 (r)",
    version: (2, 4, 2),
    build: 81,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/2.0 — mwcceppc 2.4.7 build 92.
pub const GC_2_0: CompilerBuild = CompilerBuild {
    label: "GC/2.0",
    product: "CodeWarrior for GameCube 2.0",
    version: (2, 4, 7),
    build: 92,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/2.0p1 — mwcceppc 2.4.7 build 92, patch 1. Identical to GC/2.0 except the
/// int->float conversion schedules the value store before the bias load.
pub const GC_2_0P1: CompilerBuild = CompilerBuild {
    label: "GC/2.0p1",
    product: "CodeWarrior for GameCube 2.0 (patch 1)",
    version: (2, 4, 7),
    build: 92,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc20Patch1,
};

/// GC/2.5 — mwcceppc 2.4.7 build 105.
pub const GC_2_5: CompilerBuild = CompilerBuild {
    label: "GC/2.5",
    product: "CodeWarrior for GameCube 2.5",
    version: (2, 4, 7),
    build: 105,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/2.6 — mwcceppc 2.4.7 build 107.
pub const GC_2_6: CompilerBuild = CompilerBuild {
    label: "GC/2.6",
    product: "CodeWarrior for GameCube 2.6",
    version: (2, 4, 7),
    build: 107,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/2.7 — mwcceppc 2.4.7 build 108.
pub const GC_2_7: CompilerBuild = CompilerBuild {
    label: "GC/2.7",
    product: "CodeWarrior for GameCube 2.7",
    version: (2, 4, 7),
    build: 108,
    comment_marker: 0x0b,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// Every build the generator reproduces byte-for-byte across the canary suite.
pub const SUPPORTED: &[CompilerBuild] =
    &[GC_1_3, GC_1_3_2, GC_1_3_2R, GC_2_0, GC_2_0P1, GC_2_5, GC_2_6, GC_2_7];

/// Known compiler identities whose profiles are still incomplete. They are
/// available only through the explicit experimental-build opt-in.
pub const EXPERIMENTAL: &[CompilerBuild] = &[GC_1_2_5, GC_1_2_5N];

/// The default build new compilations target until one is selected.
pub const DEFAULT: CompilerBuild = GC_1_3_2;

/// Look up a build by its decomp label (e.g. "GC/1.3.2" or the bare "1.3.2").
pub fn by_label(label: &str) -> Option<CompilerBuild> {
    SUPPORTED
        .iter()
        .copied()
        .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
}

/// Look up either a supported or explicitly experimental build.
pub fn by_label_experimental(label: &str) -> Option<CompilerBuild> {
    by_label(label).or_else(|| {
        EXPERIMENTAL
            .iter()
            .copied()
            .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_build_is_opt_in_and_keeps_distinct_object_conventions() {
        assert!(by_label("1.2.5n").is_none());
        let build = by_label_experimental("1.2.5n").expect("experimental build");
        assert_eq!(build.version, (2, 3, 3));
        assert_eq!(build.comment_version, (2, 3, 0));
        assert_eq!(build.comment_marker, 0x08);
        assert_eq!(build.emb_sda21_offset, 2);
        assert!(build.function_symbol_before_references);
        assert_eq!(build.initial_anonymous_counter, 2);
        assert_eq!(build.post_leaf_function_anonymous_bump, 1);
        assert_eq!(build.post_framed_function_anonymous_bump, 1);
    }

    #[test]
    fn supported_builds_retain_mainline_object_numbering() {
        assert!(SUPPORTED.iter().all(|build| build.emb_sda21_offset == 0));
        assert!(SUPPORTED.iter().all(|build| !build.function_symbol_before_references));
        assert!(SUPPORTED.iter().all(|build| build.initial_anonymous_counter == 5));
        assert!(SUPPORTED.iter().all(|build| build.post_leaf_function_anonymous_bump == 4));
        assert!(SUPPORTED.iter().all(|build| build.post_framed_function_anonymous_bump == 4));
    }
}
